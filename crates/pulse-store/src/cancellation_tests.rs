use super::*;
#[test]
fn cancellation_before_dispatch_never_writes() -> std::result::Result<(), Box<dyn std::error::Error>>
{
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let c = database::open(&dir.path().join("store.db"))?;
    let mut p = profiles::AutoProfilePreferences::default();
    p.suppress(profiles::AutomaticShell::Bash);
    let (reply, result) = oneshot::channel();
    drop(result);
    dispatch(&c, Request::Save(p, reply));
    assert_eq!(
        database::load(&c)?,
        profiles::AutoProfilePreferences::default()
    );
    Ok(())
}
