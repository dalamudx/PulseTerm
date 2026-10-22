use super::*;
use crate::custom_profiles::{CustomProfileDraft, CustomProfileEdit};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn custom_default_reopens_and_directory_changes_reject_stale_saves() -> TestResult {
    let temporary = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(temporary.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = temporary.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let empty = store.load_custom_profiles().await?;
    let catalog = store
        .edit_custom_profiles(
            empty,
            CustomProfileEdit::Create(CustomProfileDraft {
                name: "保存 🌍".into(),
                executable: if cfg!(windows) {
                    r"C:\tools\sh.exe"
                } else {
                    "/usr/bin/sh"
                }
                .into(),
                hidden: false,
                config: Default::default(),
            }),
            || true,
        )
        .await?;
    let id = catalog.profiles()[0].id();
    let before = store.load_default_profile().await?;
    store
        .save_default_profile_selection(before, Some(DefaultProfileSelection::Custom(id)), || true)
        .await?;
    let saved = store.load_default_profile().await?;
    assert_eq!(saved.selection(), Some(DefaultProfileSelection::Custom(id)));
    let mut changed = catalog.profiles()[0].draft().clone();
    changed.hidden = true;
    let hidden = store
        .edit_custom_profiles(catalog, CustomProfileEdit::Update(id, changed), || true)
        .await?;
    assert!(matches!(
        store
            .save_default_profile_selection(saved, None, || true)
            .await,
        Err(StoreError::Conflict)
    ));
    let current = store.load_default_profile().await?;
    assert_eq!(
        current.selection(),
        Some(DefaultProfileSelection::Custom(id))
    );
    store
        .edit_custom_profiles(hidden, CustomProfileEdit::Delete(id), || true)
        .await?;
    let snapshot = store.load_default_profile().await?;
    assert_eq!(
        snapshot.selection(),
        Some(DefaultProfileSelection::Custom(id))
    );
    assert!(
        store
            .save_default_profile_selection(
                snapshot,
                Some(DefaultProfileSelection::Custom(id)),
                || true
            )
            .await
            .is_err()
    );
    worker.shutdown().await?;
    let (reopened, worker) = StateStore::open(path).await?;
    assert_eq!(
        reopened.load_default_profile().await?.selection(),
        Some(DefaultProfileSelection::Custom(id))
    );
    worker.shutdown().await?;
    Ok(())
}
