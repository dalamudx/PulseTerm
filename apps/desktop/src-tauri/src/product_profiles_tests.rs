#![allow(clippy::unwrap_used)]
use super::*;
use pulse_app::product_document::ProductDocuments;
use pulse_store::profiles::{AutoProfileEdit, AutomaticShell};
use std::{fs, os::unix::fs::PermissionsExt};

type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn page() -> (
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
) {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().unwrap();
    documents.install(owner).unwrap();
    documents.started(owner).unwrap();
    let nonce = documents.finished(owner).unwrap();
    documents.confirm(owner, nonce).unwrap();
    let scope = documents.current(owner, nonce).unwrap();
    (registry, documents, owner, scope)
}
fn candidates(root: &std::path::Path) -> std::io::Result<OsString> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    for kind in ["bash", "zsh", "fish", "sh", "pwsh"] {
        let file = bin.join(kind);
        fs::write(&file, b"not executed")?;
        fs::set_permissions(file, fs::Permissions::from_mode(0o700))?;
    }
    Ok(bin.into_os_string())
}

#[tokio::test]
async fn real_database_visibility_and_deletion_are_read_fresh_without_paths() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, task) = StateStore::open(dir.join("state.db")).await?;
    store
        .edit_auto_profiles(AutoProfileEdit::Visibility {
            shell: AutomaticShell::Bash,
            hidden: true,
        })
        .await?;
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await?;
    for shell in [
        AutomaticShell::Zsh,
        AutomaticShell::Fish,
        AutomaticShell::Pwsh,
    ] {
        store
            .edit_auto_profiles(AutoProfileEdit::Suppress(shell))
            .await?;
    }
    let path = candidates(root.path())?;
    let (registry, _documents, _owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let offers = reader
        .list(
            registry.clone(),
            scope.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(
        serde_json::to_value(&offers)?,
        serde_json::json!([{"kind":"bash","hidden":true}])
    );
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Bash))
        .await?;
    assert!(
        reader
            .list(registry.clone(), scope, dir, path, None)
            .await?
            .is_empty()
    );
    reader.close();
    task.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}

#[tokio::test(flavor = "current_thread")]
async fn page_retirement_before_supervisor_runs_creates_no_database() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("absent");
    let (registry, documents, owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let mut work = Box::pin(reader.list(
        registry.clone(),
        scope,
        dir.clone(),
        OsString::from("/bin"),
        None,
    ));
    tokio::select! { biased; result = &mut work => panic!("unexpected result: {}", result.is_ok()), _ = std::future::ready(()) => {} }
    assert_eq!(reader.credit.available_permits(), 0);
    documents.started(owner)?;
    assert!(work.await.is_err());
    assert!(!dir.exists());
    reader.close();
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn failed_open_or_closed_store_never_returns_a_default_offer() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("public");
    fs::create_dir(&dir)?;
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755))?;
    let (registry, _documents, _owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    assert!(
        reader
            .list(
                registry.clone(),
                scope.clone(),
                dir,
                OsString::from("/bin"),
                None
            )
            .await
            .is_err()
    );
    let dir = root.path().join("private");
    reader
        .list(
            registry.clone(),
            scope.clone(),
            dir.clone(),
            OsString::from("/bin"),
            None,
        )
        .await?;
    reader.cancel_store();
    assert!(
        reader
            .list(
                registry.clone(),
                scope.clone(),
                dir.clone(),
                OsString::from("/bin"),
                None
            )
            .await
            .is_err()
    );
    reader.close();
    assert!(
        reader
            .list(registry.clone(), scope, dir, OsString::from("/bin"), None)
            .await
            .is_err()
    );
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn cancelling_blocking_wait_does_not_release_credit_until_actual_exit() -> TestResult {
    let reader = Arc::new(ProfileReader::new());
    let semaphore = reader.credit.clone();
    let credit = reader.reserve()?;
    let (entered, entering) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let waiter = tokio::spawn(blocking(credit.clone(), move || {
        let _ = entered.send(());
        released.recv().map_err(|_| "workspace.unavailable")?;
        Ok(())
    }));
    entering.await?;
    drop(credit);
    waiter.abort();
    assert!(waiter.await.is_err());
    assert!(semaphore.clone().try_acquire_owned().is_err());
    // Await semaphore acquisition rather than poll; it only resolves when the closure exits.
    release.send(())?;
    let permit = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        semaphore.clone().acquire_owned(),
    )
    .await??;
    drop(permit);
    Ok(())
}
