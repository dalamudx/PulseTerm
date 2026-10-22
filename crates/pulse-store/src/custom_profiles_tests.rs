use super::*;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn draft() -> CustomProfileDraft {
    CustomProfileDraft {
        name: "保存 🌍".into(),
        executable: if cfg!(windows) {
            r"C:\tools\sh.exe"
        } else {
            "/usr/bin/sh"
        }
        .into(),
        hidden: false,
        config: ProfileConfig {
            args: vec!["".into(), "literal ; $(text)".into()],
            env: vec![("CUSTOM_VALUE".into(), "value = 中文".into())],
            ..Default::default()
        },
    }
}
fn directory() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
#[tokio::test]
async fn reopen_cas_tombstone_restore_keep_identity_and_literals() -> TestResult {
    let temp = directory()?;
    let path = temp.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let empty = store.load_custom_profiles().await?;
    let created = store
        .edit_custom_profiles(empty.clone(), CustomProfileEdit::Create(draft()), || true)
        .await?;
    let profile = &created.profiles()[0];
    let id = profile.id();
    let first_clock = profile.identity()?.clock();
    assert!(profile.draft() == &draft());
    assert!(matches!(
        store
            .edit_custom_profiles(empty, CustomProfileEdit::Create(draft()), || true)
            .await,
        Err(StoreError::Conflict)
    ));
    let deleted = store
        .edit_custom_profiles(created.clone(), CustomProfileEdit::Delete(id), || true)
        .await?;
    assert!(deleted.active(id).is_err());
    assert!(deleted.profiles()[0].identity()?.clock() > first_clock);
    let restored = store
        .edit_custom_profiles(deleted, CustomProfileEdit::Restore(id), || true)
        .await?;
    assert!(restored.active(id)?.draft() == &draft());
    assert!(matches!(
        store
            .edit_custom_profiles(created, CustomProfileEdit::Delete(id), || true)
            .await,
        Err(StoreError::Conflict)
    ));
    task.shutdown().await?;
    let (reopened, task) = StateStore::open(path).await?;
    let actual = reopened.load_custom_profiles().await?;
    assert!(actual.active(id)?.draft() == &draft());
    assert!(matches!(
        reopened
            .edit_custom_profiles(restored, CustomProfileEdit::Delete(id), || true)
            .await,
        Err(StoreError::Conflict)
    ));
    task.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn commit_revocation_rolls_back_and_invalid_paths_or_reserved_env_fail() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let temp = directory()?;
    let (store, task) = StateStore::open(temp.path().join("state.db")).await?;
    let empty = store.load_custom_profiles().await?;
    let attempts = Arc::new(AtomicUsize::new(0));
    let gate = attempts.clone();
    assert!(matches!(
        store
            .edit_custom_profiles(
                empty.clone(),
                CustomProfileEdit::Create(draft()),
                move || gate.fetch_add(1, Ordering::SeqCst) == 0
            )
            .await,
        Err(StoreError::ProfileAuthorization)
    ));
    assert_eq!(attempts.load(Ordering::SeqCst), 2);
    assert!(store.load_custom_profiles().await? == empty);
    for executable in ["sh", "~/bin/sh", "/bin/sh\0extra"] {
        let mut invalid = draft();
        invalid.executable = executable.into();
        assert!(
            store
                .edit_custom_profiles(empty.clone(), CustomProfileEdit::Create(invalid), || true)
                .await
                .is_err()
        );
    }
    let mut invalid = draft();
    invalid.config.env = vec![("PULSE_SESSION_ID".into(), "forged".into())];
    assert!(
        store
            .edit_custom_profiles(empty.clone(), CustomProfileEdit::Create(invalid), || true)
            .await
            .is_err()
    );
    assert!(store.load_custom_profiles().await? == empty);
    task.shutdown().await?;
    Ok(())
}
