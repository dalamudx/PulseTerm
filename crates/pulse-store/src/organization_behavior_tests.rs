use super::*;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    db: Connection,
    _directory: tempfile::TempDir,
    owner: Uuid,
    host: Uuid,
    group: Uuid,
}
fn fixture() -> std::result::Result<Fixture, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let db = crate::database::open(&directory.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let hosts = crate::ssh_hosts::load(&db, owner)?;
    let hosts = crate::ssh_hosts::edit(
        &db,
        owner,
        &hosts,
        crate::ssh_hosts::HostEdit::Create(pulse_core::ssh_endpoint::SshEndpoint::new(
            "fixture",
            "fixture.invalid",
            22,
            "user",
        )?),
        || true,
    )?;
    let host = hosts.records()[0].id();
    let initial = organization::load(&db, owner)?;
    let grouped = organization::edit(
        &db,
        owner,
        &initial,
        organization::Edit::CreateGroup {
            name: "work".into(),
            parent: None,
        },
        || true,
    )?;
    let value = grouped.projection();
    let group = Uuid::parse_str(
        value["groups"][0]["metadata"]["id"]
            .as_str()
            .ok_or("group id missing")?,
    )?;
    organization::edit(
        &db,
        owner,
        &grouped,
        organization::Edit::Assign {
            kind: organization::Kind::Host,
            item: host.to_string(),
            group: Some(group.to_string()),
            tags: vec![],
            favorite: false,
        },
        || true,
    )?;
    Ok(Fixture {
        db,
        _directory: directory,
        owner,
        host,
        group,
    })
}
fn run(
    f: &Fixture,
    scope: SettingsScope,
    expected: Option<Snapshot>,
    replacement: Option<Overrides>,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<Snapshot> {
    let (reply, receive) = oneshot::channel();
    serve(
        &f.db,
        Operation {
            scope,
            expected,
            replacement,
            owner: f.owner,
            authorize,
            reply,
        },
    );
    receive.blocking_recv().map_err(|_| StoreError::Closed)?
}
fn get(f: &Fixture, scope: SettingsScope) -> Result<Snapshot> {
    run(f, scope, None, None, Arc::new(|| true))
}
fn save(
    f: &Fixture,
    scope: SettingsScope,
    expected: Snapshot,
    values: Overrides,
) -> Result<Snapshot> {
    run(f, scope, Some(expected), Some(values), Arc::new(|| true))
}
#[test]
fn organized_behavior_inherits_explicit_false_resets_and_preserves_other_fields() -> TestResult {
    let nulls = serde_json::json!({"terminal_preset":null,"term_type":null,"mac_option_as_meta":null,"backspace_send":null,"delete_send":null});
    assert!(serde_json::from_value::<Overrides>(nulls.clone()).is_ok());
    for key in [
        "terminal_preset",
        "term_type",
        "mac_option_as_meta",
        "backspace_send",
        "delete_send",
    ] {
        let mut incomplete = nulls.clone();
        incomplete
            .as_object_mut()
            .ok_or("expected object")?
            .remove(key);
        assert!(serde_json::from_value::<Overrides>(incomplete).is_err());
    }
    let f = fixture()?;
    let group = SettingsScope::Group(f.group);
    let host = SettingsScope::Host(f.host);
    let base = crate::settings::SettingsResolver::resolve(
        Some("[terminal]\ncolor_preset=\"midnight\"\n"),
        None,
        None,
    )?;
    let group_snapshot = get(&f, group)?;
    save(
        &f,
        group,
        group_snapshot,
        Overrides {
            terminal_preset: Some(crate::settings::TerminalPreset::SolarizedLight),
            term_type: Some(TerminalType::Vt100),
            mac_option_as_meta: Some(true),
            backspace_send: Some("bs".into()),
            delete_send: Some("del".into()),
        },
    )?;
    let inherited = get(&f, host)?.resolve(base.clone());
    assert_eq!(
        inherited.terminal_preset.value,
        crate::settings::TerminalPreset::SolarizedLight
    );
    assert_eq!(inherited.terminal_preset.source, SettingsSource::Group);
    assert_eq!(inherited.term_type.value, TerminalType::Vt100);
    assert_eq!(inherited.term_type.source, SettingsSource::Group);
    assert!(inherited.mac_option_as_meta.value);
    let original = crate::settings_database::load(&f.db, host)?;
    let mut patch = original.patch().clone();
    patch.terminal.history_limit = Some(5000);
    crate::settings_database::replace(&f.db, &original, patch)?;
    let host_snapshot = get(&f, host)?;
    let saved = save(
        &f,
        host,
        host_snapshot,
        Overrides {
            terminal_preset: Some(crate::settings::TerminalPreset::Dracula),
            term_type: Some(TerminalType::Xterm),
            mac_option_as_meta: Some(false),
            ..Default::default()
        },
    )?;
    let own = saved.resolve(base.clone());
    assert_eq!(
        own.terminal_preset.value,
        crate::settings::TerminalPreset::Dracula
    );
    assert_eq!(own.terminal_preset.source, SettingsSource::Host);
    assert_eq!(own.term_type.source, SettingsSource::Host);
    assert!(!own.mac_option_as_meta.value);
    assert_eq!(saved.target.patch().terminal.history_limit, Some(5000));
    let reset = save(&f, host, saved, Overrides::default())?;
    let effective = reset.resolve(base.clone());
    assert_eq!(
        effective.terminal_preset.value,
        crate::settings::TerminalPreset::SolarizedLight
    );
    assert_eq!(effective.terminal_preset.source, SettingsSource::Group);
    assert_eq!(effective.term_type.source, SettingsSource::Group);
    assert_eq!(effective.delete_send.value, "del");
    assert!(effective.mac_option_as_meta.value);
    assert_eq!(reset.target.patch().terminal.history_limit, Some(5000));
    let group_snapshot = get(&f, group)?;
    save(&f, group, group_snapshot, Overrides::default())?;
    let global = get(&f, host)?.resolve(base);
    assert_eq!(
        global.terminal_preset.value,
        crate::settings::TerminalPreset::Midnight
    );
    assert_eq!(global.terminal_preset.source, SettingsSource::User);
    assert_eq!(
        get(&f, host)?.target.patch().terminal.history_limit,
        Some(5000)
    );
    Ok(())
}
#[test]
fn organized_behavior_rejects_parent_revision_and_membership_changes() -> TestResult {
    let f = fixture()?;
    let host = SettingsScope::Host(f.host);
    let group = SettingsScope::Group(f.group);
    let stale = get(&f, host)?;
    let group_snapshot = get(&f, group)?;
    save(
        &f,
        group,
        group_snapshot,
        Overrides {
            delete_send: Some("del".into()),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        save(&f, host, stale, Overrides::default()),
        Err(StoreError::SettingsConflict)
    ));
    let stale = get(&f, host)?;
    let org = organization::load(&f.db, f.owner)?;
    organization::edit(
        &f.db,
        f.owner,
        &org,
        organization::Edit::Assign {
            kind: organization::Kind::Host,
            item: f.host.to_string(),
            group: None,
            tags: vec![],
            favorite: false,
        },
        || true,
    )?;
    assert!(matches!(
        save(&f, host, stale, Overrides::default()),
        Err(StoreError::SettingsConflict)
    ));
    let stale = get(&f, host)?;
    let fresh = save(
        &f,
        host,
        stale.clone(),
        Overrides {
            backspace_send: Some("bs".into()),
            ..Default::default()
        },
    )?;
    assert!(matches!(
        save(&f, host, stale, Overrides::default()),
        Err(StoreError::SettingsConflict)
    ));
    assert!(matches!(
        save(&f, group, fresh, Overrides::default()),
        Err(StoreError::SettingsConflict)
    ));
    assert!(get(&f, SettingsScope::Host(Uuid::now_v7())).is_err());
    Ok(())
}
#[test]
fn organized_behavior_revocation_rolls_back_and_cancelled_read_does_no_work() -> TestResult {
    let f = fixture()?;
    let scope = SettingsScope::Host(f.host);
    let snapshot = get(&f, scope)?;
    let revision = snapshot.target.revision();
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let guard_calls = calls.clone();
    let guard =
        Arc::new(move || guard_calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0);
    assert!(matches!(
        run(
            &f,
            scope,
            Some(snapshot),
            Some(Overrides {
                delete_send: Some("del".into()),
                ..Default::default()
            }),
            guard
        ),
        Err(StoreError::ProfileAuthorization)
    ));
    assert_eq!(get(&f, scope)?.target.revision(), revision);
    assert_eq!(get(&f, scope)?.overrides(), Overrides::default());
    let (reply, receive) = oneshot::channel();
    drop(receive);
    serve(
        &f.db,
        Operation {
            scope,
            expected: None,
            replacement: None,
            owner: f.owner,
            authorize: Arc::new(|| panic!("cancelled work must not start")),
            reply,
        },
    );
    assert!(serde_json::from_value::<Overrides>(serde_json::json!({"term_type":"bad"})).is_err());
    assert!(serde_json::from_value::<Overrides>(serde_json::json!({"unknown":true})).is_err());
    let snapshot = get(&f, scope)?;
    assert!(
        save(
            &f,
            scope,
            snapshot,
            Overrides {
                backspace_send: Some("bad".into()),
                ..Default::default()
            }
        )
        .is_err()
    );
    Ok(())
}
