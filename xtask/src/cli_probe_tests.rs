#![allow(clippy::unwrap_used)]
use super::*;
#[test]
fn flags_alone_cannot_authorize_cli_or_claim_os_permissions() {
    assert!(validate(&json!({"schemaVersion":1,"completed":true})).is_err());
}
#[test]
fn private_token_requires_current_directory_modes_and_non_symlink() {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = d.path().join("token");
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&path)
        .unwrap();
    f.write_all(&[b'a'; 64]).unwrap();
    drop(f);
    assert!(private_token(&path).is_ok());
    let link = d.path().join("link");
    std::os::unix::fs::symlink(&path, &link).unwrap();
    assert!(private_token(&link).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o640)).unwrap();
    assert!(private_token(&path).is_err());
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o750)).unwrap();
    assert!(private_token(&path).is_err());
}
