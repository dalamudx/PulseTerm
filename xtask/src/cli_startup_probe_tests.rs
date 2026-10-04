#![allow(clippy::unwrap_used)]
use super::*;
fn root() -> tempfile::TempDir {
    let d = tempfile::tempdir().unwrap();
    std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    d
}
#[test]
fn recovery_never_removes_unknown_regular_or_symlink_socket_paths() {
    let d = root();
    let p = d.path().join("rpc.sock");
    std::fs::write(&p, b"not an application socket").unwrap();
    assert!(clean_owned_endpoint(d.path()).is_err());
    assert_eq!(std::fs::read(&p).unwrap(), b"not an application socket");
    std::fs::remove_file(&p).unwrap();
    let other = d.path().join("unrelated");
    std::fs::write(&other, b"data").unwrap();
    std::os::unix::fs::symlink(&other, &p).unwrap();
    assert!(clean_owned_endpoint(d.path()).is_err());
    assert!(
        std::fs::symlink_metadata(&p)
            .unwrap()
            .file_type()
            .is_symlink()
    );
}
#[test]
fn token_checks_finish_before_unlinking_owned_stale_socket() {
    let d = root();
    let p = d.path().join("rpc.sock");
    let listener = std::os::unix::net::UnixListener::bind(&p).unwrap();
    std::fs::set_permissions(&p, std::fs::Permissions::from_mode(0o600)).unwrap();
    drop(listener);
    let token = d.path().join("token");
    let mut f = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&token)
        .unwrap();
    f.write_all(&[b'a'; 64]).unwrap();
    drop(f);
    let link = d.path().join("linked-token");
    std::fs::hard_link(&token, &link).unwrap();
    assert!(clean_owned_endpoint(d.path()).is_err());
    assert!(p.exists() && token.exists());
    std::fs::remove_file(&link).unwrap();
    clean_owned_endpoint(d.path()).unwrap();
    assert!(!p.exists() && !token.exists());
}
#[test]
fn flags_alone_cannot_prove_real_candidate_crash_or_fresh_token() {
    assert!(validate(&json!({"schemaVersion":1,"completed":true})).is_err());
}
