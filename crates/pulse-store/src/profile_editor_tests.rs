use super::*;
use crate::profile_default::ProfileLifecycleEdit;
use crate::profiles::AutomaticShell as Shell;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn config() -> ProfileConfig {
    ProfileConfig {
        args: vec!["".into(), " ; $(literal) 中文🌍 ".into()],
        cwd: Some(if cfg!(windows) { r"C:\data" } else { "/data" }.into()),
        env: vec![("PROFILE_VALUE".into(), "space = 中文".into())],
        env_remove: vec!["INHERITED_VALUE".into()],
        appearance: ProfileAppearance {
            font_size: Some(18),
            font_family: Some("Pulse Mono".into()),
            cursor_blink: Some(false),
            mac_option_is_meta: Some(false),
            backspace_send: Some("bs".into()),
            delete_send: Some("del".into()),
            color_preset: Some(crate::settings::TerminalPreset::OneDark),
        },
    }
}
fn dir() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
#[tokio::test]
async fn exact_configuration_reopens_and_stale_foreign_sources_fail() -> TestResult {
    let temp = dir()?;
    let path = temp.path().join("state.db");
    let (a, ta) = StateStore::open(path.clone()).await?;
    let (b, tb) = StateStore::open(path.clone()).await?;
    let old_default = a.load_default_profile().await?;
    let old = a.load_profile_editor(Shell::Bash).await?;
    let foreign = b.load_profile_editor(Shell::Bash).await?;
    assert!(matches!(
        a.save_profile_editor(foreign, config(), || true).await,
        Err(StoreError::Conflict)
    ));
    let saved = a
        .save_profile_editor(old.clone(), config(), || true)
        .await?;
    assert_eq!(saved.config(), config());
    assert!(matches!(
        a.save_default_profile(old_default, None, || true).await,
        Err(StoreError::Conflict)
    ));
    assert!(matches!(
        a.save_profile_editor(old, config(), || true).await,
        Err(StoreError::Conflict)
    ));
    drop(a);
    drop(b);
    ta.join().await?;
    tb.join().await?;
    let (store, task) = StateStore::open(path).await?;
    assert_eq!(
        store.load_profile_editor(Shell::Bash).await?.config(),
        config()
    );
    assert!(matches!(
        store.save_profile_editor(saved, config(), || true).await,
        Err(StoreError::Conflict)
    ));
    task.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn delete_restore_cannot_revive_old_editor_source() -> TestResult {
    let temp = dir()?;
    let (store, task) = StateStore::open(temp.path().join("state.db")).await?;
    let old = store.load_profile_editor(Shell::Sh).await?;
    let lifecycle = store.load_default_profile().await?;
    let deleted = store
        .change_profile_lifecycle(lifecycle, ProfileLifecycleEdit::Delete(Shell::Sh), || true)
        .await?;
    let deleted_editor = store.load_profile_editor(Shell::Sh).await?;
    assert!(matches!(
        store
            .save_profile_editor(deleted_editor, config(), || true)
            .await,
        Err(StoreError::Record)
    ));
    store
        .change_profile_lifecycle(deleted, ProfileLifecycleEdit::Restore(Shell::Sh), || true)
        .await?;
    assert!(matches!(
        store.save_profile_editor(old, config(), || true).await,
        Err(StoreError::Conflict)
    ));
    let old = store.load_profile_editor(Shell::Sh).await?;
    store
        .edit_auto_profiles(crate::profiles::AutoProfileEdit::Visibility {
            shell: Shell::Sh,
            hidden: true,
        })
        .await?;
    assert!(matches!(
        store.save_profile_editor(old, config(), || true).await,
        Err(StoreError::Conflict)
    ));
    task.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn authorization_rechecked_before_commit_rolls_back_both_records() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let temp = dir()?;
    let (store, task) = StateStore::open(temp.path().join("state.db")).await?;
    let old = store.load_profile_editor(Shell::Fish).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let gate = calls.clone();
    assert!(matches!(
        store
            .save_profile_editor(old.clone(), config(), move || gate
                .fetch_add(1, Ordering::SeqCst)
                < 2)
            .await,
        Err(StoreError::ProfileAuthorization)
    ));
    assert!(calls.load(Ordering::SeqCst) >= 3);
    assert!(store.load_profile_editor(Shell::Fish).await? == old);
    task.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn corrupt_or_future_configuration_is_not_replaced_by_defaults() -> TestResult {
    let temp = dir()?;
    let path = temp.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let external = rusqlite::Connection::open(path)?;
    let document = serde_json::json!({"schemaVersion":1,"revision":1,"config":config()});
    for value in [
        "broken".to_owned(),
        "{}".to_owned(),
        {
            let mut future = document.clone();
            future["schemaVersion"] = serde_json::json!(2);
            future.to_string()
        },
        {
            let mut unsafe_value = document.clone();
            unsafe_value["config"]["env"] = serde_json::json!([["PULSE_SOCKET", "secret"]]);
            unsafe_value.to_string()
        },
        {
            let mut unknown = document.clone();
            unknown["extra"] = serde_json::json!(true);
            unknown.to_string()
        },
    ] {
        external.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![key(Shell::Sh),&value])?;
        assert!(matches!(
            store.load_profile_editor(Shell::Sh).await,
            Err(StoreError::Record)
        ));
        assert_eq!(
            external.query_row(
                "SELECT value FROM app_state WHERE key=?1",
                [key(Shell::Sh)],
                |row| row.get::<_, String>(0)
            )?,
            value
        );
    }
    external.execute("DELETE FROM app_state WHERE key=?1", [key(Shell::Sh)])?;
    assert_eq!(
        store.load_profile_editor(Shell::Sh).await?.config(),
        ProfileConfig::default()
    );
    drop(external);
    task.shutdown().await?;
    Ok(())
}
#[test]
fn literal_bounds_reserved_identity_and_sparse_appearance_are_validated() {
    let valid = config();
    assert!(valid.validate().is_ok());
    for name in [
        "TERM_PROGRAM",
        "term_program_version",
        "Pulse_Socket",
        "PULSE_PANE_ID",
        "pulse_future_authority",
        "BAD=KEY",
        "0KEY",
        "",
    ] {
        let mut c = valid.clone();
        c.env = vec![(name.into(), "secret".into())];
        assert!(c.validate().is_err());
        c.env.clear();
        c.env_remove = vec![name.into()];
        assert!(c.validate().is_err());
    }
    let mut c = valid.clone();
    c.args = vec!["".into(); 64];
    assert!(c.validate().is_ok());
    c.args.push("".into());
    assert!(c.validate().is_err());
    c = valid.clone();
    c.args = vec!["x".repeat(16384)];
    assert!(c.validate().is_err());
    c = valid.clone();
    c.cwd = Some("~/not-expanded".into());
    assert!(c.validate().is_err());
    c = valid.clone();
    c.env.push(("profile_value".into(), "duplicate".into()));
    assert!(c.validate().is_err());
    c = valid.clone();
    c.args.push("nul\0".into());
    assert!(c.validate().is_err());
    c = valid.clone();
    c.appearance.font_family = Some("x;url(secret)".into());
    assert!(c.validate().is_err());
    c = valid.clone();
    c.appearance.font_size = Some(73);
    assert!(c.validate().is_err());
    c = ProfileConfig::default();
    c.args = vec!["a".repeat(16380)];
    c.appearance.font_family = Some("Mono".into());
    assert!(c.validate().is_err());
    let debug = format!("{valid:?}");
    for text in ["literal", "PROFILE_VALUE", "/data", "space ="] {
        assert!(!debug.contains(text));
    }
}
