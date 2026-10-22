use crate::{
    Request, StateStore, StoreError, dispatch,
    settings::{SettingsError, SettingsPatch, SettingsSource},
    settings_database as db,
    settings_edit::TerminalSettingEdit as Set,
    settings_remove::TerminalSettingKey as Key,
    settings_scope::*,
};
use rusqlite::{Connection, Transaction, TransactionBehavior};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn directory() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn connection() -> Result<(tempfile::TempDir, Connection), Box<dyn std::error::Error>> {
    let dir = directory()?;
    let connection = crate::database::open(&dir.path().join("state.db"))?;
    Ok((dir, connection))
}
fn group() -> SettingsScope {
    SettingsScope::Group(Uuid::from_u128(1))
}
fn host() -> SettingsScope {
    SettingsScope::Host(Uuid::from_u128(1))
}
fn size(size: u16) -> ScopedSettingsEdit {
    ScopedSettingsEdit::Set(Set::FontSize(size))
}
fn store_record(c: &Connection, scope: SettingsScope, text: &str) -> TestResult {
    c.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![scope.key()?,text])?;
    Ok(())
}
#[test]
fn same_uuid_group_and_host_are_distinct_and_nil_is_not_default_scope() -> TestResult {
    let (_dir, c) = connection()?;
    assert_eq!(db::load(&c, group())?.revision(), 0);
    db::edit(&c, group(), size(18))?;
    db::edit(&c, host(), size(22))?;
    assert_eq!(db::load(&c, group())?.patch().terminal.font_size, Some(18));
    assert_eq!(db::load(&c, host())?.patch().terminal.font_size, Some(22));
    assert!(matches!(
        db::load(&c, SettingsScope::Group(Uuid::nil())),
        Err(StoreError::SettingsRecord)
    ));
    Ok(())
}
#[tokio::test]
async fn independent_workers_concurrently_edit_different_fields_without_loss() -> TestResult {
    let dir = directory()?;
    let path = dir.path().join("state.db");
    let (a, ta) = StateStore::open(path.clone())
        .await
        .map_err(|error| format!("settings.concurrent.open_a: {error}"))?;
    let (b, tb) = StateStore::open(path)
        .await
        .map_err(|error| format!("settings.concurrent.open_b: {error}"))?;
    let (one, two) = tokio::join!(
        a.edit_scoped_settings(group(), size(18)),
        b.edit_scoped_settings(group(), ScopedSettingsEdit::Set(Set::CursorBlink(false)))
    );
    one.map_err(|error| format!("settings.concurrent.edit_a: {error}"))?;
    two.map_err(|error| format!("settings.concurrent.edit_b: {error}"))?;
    let snapshot = a
        .load_scoped_settings(group())
        .await
        .map_err(|error| format!("settings.concurrent.load: {error}"))?;
    assert_eq!(snapshot.revision(), 2);
    assert_eq!(snapshot.patch().terminal.font_size, Some(18));
    assert_eq!(snapshot.patch().terminal.cursor_blink, Some(false));
    ta.shutdown().await?;
    tb.shutdown().await?;
    Ok(())
}
#[test]
fn aba_same_values_does_not_reactivate_old_revision() -> TestResult {
    let (_dir, c) = connection()?;
    let old = db::edit(&c, group(), size(18))?;
    db::edit(&c, group(), size(20))?;
    let current = db::edit(&c, group(), size(18))?;
    assert_eq!(old.patch(), current.patch());
    assert_eq!(current.revision(), 3);
    assert!(matches!(
        db::replace(&c, &old, SettingsPatch::default()),
        Err(StoreError::SettingsConflict)
    ));
    assert_eq!(db::load(&c, group())?.revision(), 3);
    Ok(())
}
#[test]
fn deleting_last_field_retains_tombstone_and_noop_does_not_advance() -> TestResult {
    let (_dir, c) = connection()?;
    let initial = db::load(&c, group())?;
    let noop = db::edit(&c, group(), ScopedSettingsEdit::Remove(Key::FontSize))?;
    assert_eq!(noop.revision(), 0);
    db::edit(&c, group(), size(18))?;
    let empty = db::edit(&c, group(), ScopedSettingsEdit::Remove(Key::FontSize))?;
    assert_eq!(empty.revision(), 2);
    assert_eq!(empty.patch(), &SettingsPatch::default());
    assert!(matches!(
        db::replace(&c, &initial, SettingsPatch::default()),
        Err(StoreError::SettingsConflict)
    ));
    let noop = db::edit(&c, group(), ScopedSettingsEdit::Remove(Key::FontSize))?;
    assert_eq!(noop.revision(), 2);
    Ok(())
}
#[test]
fn invalid_edits_never_mutate_valid_record_or_revision() -> TestResult {
    let (_dir, c) = connection()?;
    db::edit(&c, group(), size(18))?;
    for edit in [
        size(0),
        size(73),
        ScopedSettingsEdit::Set(Set::FontFamily("x;url(secret)".into())),
    ] {
        assert!(matches!(
            db::edit(&c, group(), edit),
            Err(StoreError::SettingsRecord)
        ));
    }
    let current = db::load(&c, group())?;
    assert_eq!(current.revision(), 1);
    assert_eq!(current.patch().terminal.font_size, Some(18));
    Ok(())
}
#[test]
fn malformed_future_misbound_and_unknown_documents_never_become_empty() -> TestResult {
    let (_dir, c) = connection()?;
    let good = db::edit(&c, group(), size(18))?.encode()?;
    let base: serde_json::Value = serde_json::from_str(&good)?;
    let mut cases = vec!["bad".to_owned(), "x".repeat(8193)];
    for (key, value) in [
        ("schemaVersion", serde_json::json!(2)),
        ("kind", serde_json::json!("host")),
        ("id", serde_json::json!(Uuid::from_u128(2).to_string())),
        ("revision", serde_json::json!(0)),
        ("revision", serde_json::json!(u64::MAX)),
        ("secret", serde_json::json!("never_echo")),
    ] {
        let mut doc = base.clone();
        doc[key] = value;
        cases.push(serde_json::to_string(&doc)?);
    }
    cases.push(format!(
        "{{\"schemaVersion\":1,{}",
        good.trim_start_matches('{')
    ));
    for text in cases {
        store_record(&c, group(), &text)?;
        assert!(matches!(
            db::load(&c, group()),
            Err(StoreError::SettingsRecord)
        ));
        assert!(matches!(
            db::edit(&c, group(), size(20)),
            Err(StoreError::SettingsRecord)
        ));
        let persisted: String = c.query_row(
            "SELECT value FROM app_state WHERE key=?1",
            [group().key()?],
            |row| row.get(0),
        )?;
        assert_eq!(persisted, text);
    }
    Ok(())
}
#[test]
fn maximum_revision_rejects_changes_but_allows_idempotent_read() -> TestResult {
    let (_dir, c) = connection()?;
    let mut current = db::edit(&c, group(), size(18))?;
    current.revision = i64::MAX as u64;
    store_record(&c, group(), &current.encode()?)?;
    assert!(matches!(
        db::edit(&c, group(), size(20)),
        Err(StoreError::SettingsRevision)
    ));
    assert_eq!(db::edit(&c, group(), size(18))?.revision(), i64::MAX as u64);
    assert_eq!(db::load(&c, group())?.patch().terminal.font_size, Some(18));
    Ok(())
}
#[test]
fn paired_read_transaction_cannot_mix_old_group_with_new_host() -> TestResult {
    let (dir, c) = connection()?;
    let writer = crate::database::open(&dir.path().join("state.db"))?;
    db::edit(&c, group(), size(18))?;
    db::edit(&c, host(), size(20))?;
    let transaction = Transaction::new_unchecked(&c, TransactionBehavior::Deferred)?;
    let first = db::load(&transaction, group())?;
    assert_eq!(first.revision(), 1);
    db::edit(&writer, host(), size(22))?;
    let layers = db::read_layers(
        &transaction,
        Some(Uuid::from_u128(1)),
        Some(Uuid::from_u128(1)),
    )?;
    assert_eq!(
        layers
            .host()
            .ok_or("missing host")?
            .patch()
            .terminal
            .font_size,
        Some(20)
    );
    transaction.commit()?;
    assert_eq!(db::load(&c, host())?.patch().terminal.font_size, Some(22));
    Ok(())
}
#[test]
fn dropped_settings_replies_cancel_before_dispatch_without_writing() -> TestResult {
    let (_dir, c) = connection()?;
    let old = db::load(&c, group())?;
    let (reply, result) = tokio::sync::oneshot::channel();
    drop(result);
    dispatch(&c, Request::SettingsEdit(group(), size(18), reply));
    let (reply, result) = tokio::sync::oneshot::channel();
    drop(result);
    dispatch(
        &c,
        Request::SettingsReplace(old, SettingsPatch::parse("terminal.font_size=20")?, reply),
    );
    assert_eq!(db::load(&c, group())?.revision(), 0);
    Ok(())
}
#[test]
fn typed_layers_validate_low_priority_and_preserve_per_field_source() -> TestResult {
    let (_dir, c) = connection()?;
    db::edit(&c, group(), size(18))?;
    db::edit(&c, host(), ScopedSettingsEdit::Set(Set::CursorBlink(false)))?;
    let user = SettingsPatch::parse("terminal.font_family='Fira Code'")?;
    let layers = db::layers(&c, Some(Uuid::from_u128(1)), Some(Uuid::from_u128(1)))?;
    let resolved = layers.resolve(Some(&user))?;
    assert_eq!(resolved.font_size.source, SettingsSource::Group);
    assert_eq!(resolved.font_family.source, SettingsSource::User);
    assert_eq!(resolved.cursor_blink.source, SettingsSource::Host);
    let mut invalid = SettingsPatch::default();
    invalid.terminal.font_size = Some(999);
    assert_eq!(layers.resolve(Some(&invalid)), Err(SettingsError::Value));
    db::edit(&c, group(), ScopedSettingsEdit::Remove(Key::FontSize))?;
    let empty = db::layers(&c, Some(Uuid::from_u128(1)), None)?;
    assert_eq!(
        empty.resolve(Some(&user))?.font_size.source,
        SettingsSource::Default
    );
    Ok(())
}
#[tokio::test]
async fn worker_reopen_persists_scopes_without_touching_profile_preferences() -> TestResult {
    let dir = directory()?;
    let path = dir.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    store.edit_scoped_settings(group(), size(18)).await?;
    store.edit_scoped_settings(host(), size(22)).await?;
    task.shutdown().await?;
    assert!(matches!(
        store.load_settings_layers(None, None).await,
        Err(StoreError::Closed)
    ));
    let (reopened, task) = StateStore::open(path).await?;
    let layers = reopened
        .load_settings_layers(Some(Uuid::from_u128(1)), Some(Uuid::from_u128(1)))
        .await?;
    assert_eq!(layers.group().ok_or("group missing")?.revision(), 1);
    assert_eq!(
        layers
            .host()
            .ok_or("host missing")?
            .patch()
            .terminal
            .font_size,
        Some(22)
    );
    assert_eq!(
        reopened.load_auto_profiles().await?,
        crate::profiles::AutoProfilePreferences::default()
    );
    task.shutdown().await?;
    Ok(())
}

#[test]
fn ui_appearance_is_rejected_by_scoped_edits_and_replacements() -> TestResult {
    use crate::settings::UiAppearance;
    let (_dir, c) = connection()?;
    for scope in [group(), host()] {
        let before = db::load(&c, scope)?;
        assert!(
            db::edit(
                &c,
                scope,
                ScopedSettingsEdit::Set(Set::UiAppearance(UiAppearance::Dark))
            )
            .is_err()
        );
        assert!(db::edit(&c, scope, ScopedSettingsEdit::Remove(Key::UiAppearance)).is_err());
        assert!(db::replace(&c, &before, SettingsPatch::parse("ui.appearance='dark'")?).is_err());
        assert!(
            db::edit(
                &c,
                scope,
                ScopedSettingsEdit::Set(Set::UiAccent(crate::settings::UiAccent::Blue))
            )
            .is_err()
        );
        assert!(db::edit(&c, scope, ScopedSettingsEdit::Remove(Key::UiAccent)).is_err());
        assert!(db::replace(&c, &before, SettingsPatch::parse("ui.accent='blue'")?).is_err());
        let after = db::load(&c, scope)?;
        assert_eq!(after.revision(), before.revision());
        assert_eq!(after.patch(), before.patch());
    }
    Ok(())
}

#[test]
fn mac_option_scoped_overrides_persist_explicit_false_and_reset() -> TestResult {
    let (_dir, c) = connection()?;
    for scope in [group(), host()] {
        db::edit(&c, scope, size(18))?;
        db::edit(
            &c,
            scope,
            ScopedSettingsEdit::Set(Set::MacOptionAsMeta(true)),
        )?;
        assert_eq!(
            db::load(&c, scope)?.patch().terminal.mac_option_as_meta,
            Some(true)
        );
        db::edit(
            &c,
            scope,
            ScopedSettingsEdit::Set(Set::MacOptionAsMeta(false)),
        )?;
        assert_eq!(
            db::load(&c, scope)?.patch().terminal.mac_option_as_meta,
            Some(false)
        );
        db::edit(&c, scope, ScopedSettingsEdit::Remove(Key::MacOptionAsMeta))?;
        let after = db::load(&c, scope)?;
        assert_eq!(after.patch().terminal.mac_option_as_meta, None);
        assert_eq!(after.patch().terminal.font_size, Some(18));
    }
    Ok(())
}
