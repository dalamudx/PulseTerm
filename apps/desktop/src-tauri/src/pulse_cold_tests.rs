#![allow(clippy::unwrap_used)]
use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};
fn fixture() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}
#[test]
fn cold_endpoint_observation_never_creates_or_repairs_missing_or_partial_files() {
    let temp = fixture();
    let directory = temp.path().join("cli");
    assert!(matches!(endpoint(&directory), Ok(Endpoint::Missing)));
    assert!(!directory.exists());
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(matches!(endpoint(&directory), Ok(Endpoint::Missing)));
    fs::write(directory.join("control.token"), "bad").unwrap();
    assert!(endpoint(&directory).is_err());
    assert_eq!(fs::read(directory.join("control.token")).unwrap(), b"bad");
    assert!(!directory.join("control.sock").exists());
}
#[test]
fn cold_runtime_refuses_symlinks_and_unsafe_mode_without_changing_them() {
    let temp = fixture();
    let directory = temp.path().join("cli");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(endpoint(&directory).is_err());
    assert_eq!(fs::metadata(&directory).unwrap().mode() & 0o777, 0o755);
    let link = temp.path().join("link");
    symlink(&directory, &link).unwrap();
    assert!(endpoint(&link).is_err());
    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert!(endpoint(Path::new("relative/cli")).is_err());
}
#[test]
fn cold_sibling_is_fixed_and_rejects_missing_symlink_hardlink_setid_and_unsafe_modes() {
    let temp = fixture();
    let executable = temp.path().join("pulse");
    let gui = temp.path().join("pulseterm-workspace");
    assert!(gui_sibling(&executable).is_err());
    fs::write(&gui, "test fixture, never executed").unwrap();
    for mode in [0o600, 0o777, 0o4755, 0o2755] {
        fs::set_permissions(&gui, fs::Permissions::from_mode(mode)).unwrap();
        assert!(gui_sibling(&executable).is_err());
    }
    fs::set_permissions(&gui, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(gui_sibling(&executable).unwrap(), gui);
    let link = temp.path().join("hardlink");
    fs::hard_link(&gui, &link).unwrap();
    assert!(gui_sibling(&executable).is_err());
    fs::remove_file(link).unwrap();
    let real = temp.path().join("real");
    fs::rename(&gui, &real).unwrap();
    symlink(&real, &gui).unwrap();
    assert!(gui_sibling(&executable).is_err());
}
#[tokio::test(flavor = "current_thread")]
async fn blocked_cold_actions_and_invalid_parameters_do_not_write_runtime() {
    let temp = fixture();
    let directory = temp.path().join("cli");
    for (method, params) in [
        ("workspace.profiles", json!({"program":"/bin/sh"})),
        (
            "workspace.new-tab",
            json!({"profile":"sh","program":"/bin/sh"}),
        ),
        (
            "workspace.split",
            json!({"profile":"sh","direction":"diagonal"}),
        ),
        (
            "workspace.split",
            json!({"profile":"sh","direction":"column","program":"/bin/sh"}),
        ),
        ("workspace.ssh", json!({})),
        ("workspace.focus", json!({"program":"/bin/sh"})),
    ] {
        assert!(dispatch(&directory, method, params).await.is_err());
        assert!(!directory.exists());
    }
}
#[tokio::test(flavor = "current_thread")]
async fn existing_untrusted_endpoints_never_trigger_cold_launch_or_repair() {
    let temp = fixture();
    let directory = temp.path().join("cli");
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    fs::write(directory.join("control.sock"), "not a socket").unwrap();
    fs::write(directory.join("control.token"), "bad token").unwrap();
    assert!(
        dispatch(&directory, "workspace.focus", json!({}))
            .await
            .is_err()
    );
    assert_eq!(
        fs::read(directory.join("control.sock")).unwrap(),
        b"not a socket"
    );
    assert_eq!(
        fs::read(directory.join("control.token")).unwrap(),
        b"bad token"
    );
}
