use super::super::alias::resolve;
use super::*;
fn capture(f: &Fixture) -> std::result::Result<SshKeyTargetSource, Box<dyn std::error::Error>> {
    Ok(ssh_key_target_source::finish_read(
        &f.db,
        f.owner,
        ssh_target_source::read(&f.db, f.owner)?,
    )?)
}
#[test]
fn saved_alias_exact_unique_snapshot_defaults_and_no_sql_writes() -> TestResult {
    let f = fixture()?;
    let source = capture(&f)?;
    let before = app_state(&f.db)?;
    let outer = resolve(f.owner, &source, "outer")?;
    assert_eq!(outer.host_id(), f.entrances[0]);
    assert_eq!(outer.profile_id(), f.profiles[0]);
    assert!(matches!(outer.identity(), SshRouteIdentity::Password(id) if id == f.password));
    let middle = resolve(f.owner, &source, "middle")?;
    assert!(matches!(middle.identity(), SshRouteIdentity::PrivateKey(id) if id == f.key));
    for name in [
        "Outer",
        "out",
        "outer ",
        " outer",
        "missing",
        "user@unrecorded.invalid",
        "outer\u{200b}",
    ] {
        assert!(resolve(f.owner, &source, name).is_err());
    }
    assert!(resolve(Uuid::new_v4(), &source, "outer").is_err());
    assert_eq!(before, app_state(&f.db)?);
    Ok(())
}
#[test]
fn saved_alias_ambiguous_entrances_and_missing_default_reject() -> TestResult {
    let f = fixture()?;
    ssh_hosts::edit(
        &f.db,
        f.owner,
        &ssh_hosts::load(&f.db, f.owner)?,
        HostEdit::Create(SshEndpoint::new(
            "outer",
            "different.example",
            2222,
            "route-user",
        )?),
        || true,
    )?;
    assert!(resolve(f.owner, &capture(&f)?, "outer").is_err());
    let snapshot = ssh_profiles::load(&f.db, f.owner)?;
    ssh_profiles::edit(
        &f.db,
        f.owner,
        &snapshot,
        ProfileMutation::Preferences(ProfileEdit {
            id: f.profiles[2],
            label: "destination".into(),
            username: "route-user".into(),
            identity: None,
        }),
        || true,
    )?;
    assert!(resolve(f.owner, &capture(&f)?, "destination").is_err());
    Ok(())
}
#[test]
fn saved_alias_frozen_metadata_does_not_reread_rename_or_new_default() -> TestResult {
    let f = fixture()?;
    let original = capture(&f)?;
    ssh_hosts::edit(
        &f.db,
        f.owner,
        &ssh_hosts::load(&f.db, f.owner)?,
        HostEdit::Update(
            f.entrances[2],
            SshEndpoint::new("renamed", "destination.example", 2202, "route-user")?,
        ),
        || true,
    )?;
    let snapshot = ssh_profiles::load(&f.db, f.owner)?;
    ssh_profiles::edit(
        &f.db,
        f.owner,
        &snapshot,
        ProfileMutation::Preferences(ProfileEdit {
            id: f.profiles[2],
            label: "destination".into(),
            username: "route-user".into(),
            identity: Some(f.key),
        }),
        || true,
    )?;
    let selected = resolve(f.owner, &original, "destination")?;
    assert_eq!(selected.host_id(), f.entrances[2]);
    assert!(matches!(selected.identity(), SshRouteIdentity::Password(id) if id == f.password));
    assert!(resolve(f.owner, &capture(&f)?, "destination").is_err());
    assert!(
        matches!(resolve(f.owner, &capture(&f)?, "renamed")?.identity(), SshRouteIdentity::PrivateKey(id) if id == f.key)
    );
    Ok(())
}
