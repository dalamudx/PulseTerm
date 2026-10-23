//! Backend prepared private source → fresh stored root startup; not a page command.
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_app::product_document::ProductDocuments;
use pulse_app::{
    local_launch::{LocalStartError, StoredProfileLaunch},
    local_session_root::{LocalSessionRoot, start_stored_page_local},
};
use pulse_pty::{launch::LocalSessionEnvironment, profiles::ShellKind};
use pulse_store::profiles::{AutoProfileEdit, AutomaticShell};
use std::{fs, os::unix::fs::PermissionsExt, sync::Mutex};
use tokio::{
    runtime::Handle,
    time::{Duration, timeout},
};

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
fn shell(root: &std::path::Path) -> std::io::Result<OsString> {
    let bin = root.join("bin");
    fs::create_dir(&bin)?;
    let file = bin.join("sh");
    // Fixed noninteractive fixture: never executes user startup files.
    fs::write(&file, b"#!/bin/sh\nexec /usr/bin/true\n")?;
    fs::set_permissions(file, fs::Permissions::from_mode(0o700))?;
    Ok(bin.into_os_string())
}

#[tokio::test]
async fn supervised_product_start_uses_fresh_store_and_rejects_retired_page_before_spawn()
-> TestResult {
    let temp = tempfile::tempdir()?;
    let path = shell(temp.path())?;
    let (registry, documents, owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        registry.clone(),
        Handle::current(),
    )));
    let source = || ProfileStart {
        cwd: None,
        preset: pulse_store::settings::TerminalPreset::Classic.into(),
        directory: temp.path().join("state"),
        path: path.clone(),
        shell: None,
        kind: ShellKind::Sh,
    };
    let id = reader
        .start(registry.clone(), scope.clone(), root.clone(), source())
        .await?;
    let receipt = root.lock().unwrap().take_for_finish(id)?;
    assert_eq!(
        timeout(Duration::from_secs(5), receipt.finish())
            .await??
            .native
            .exit_code,
        0
    );
    root.lock().unwrap().collect_reaped();
    reader
        .store
        .get()
        .unwrap()
        .store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await?;
    assert!(
        reader
            .start(registry.clone(), scope.clone(), root.clone(), source())
            .await
            .is_err()
    );
    assert_eq!(root.lock().unwrap().occupied(), 0);
    documents.started(owner)?;
    assert!(
        reader
            .start(registry.clone(), scope, root.clone(), source())
            .await
            .is_err()
    );
    assert_eq!(root.lock().unwrap().occupied(), 0);
    reader.shutdown().await?;
    let drain = root.lock().unwrap().shutdown();
    drain.join().await?;
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn cache_does_not_silently_substitute_another_directory() -> TestResult {
    let temp = tempfile::tempdir()?;
    let path = shell(temp.path())?;
    let (registry, _documents, _owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let a = temp.path().join("a");
    let b = temp.path().join("b");
    reader
        .list(
            registry.clone(),
            scope.clone(),
            a.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert!(matches!(
        reader
            .list(
                registry.clone(),
                scope.clone(),
                b.clone(),
                path.clone(),
                None
            )
            .await,
        Err("workspace.denied")
    ));
    assert!(!b.exists());
    reader.list(registry.clone(), scope, a, path, None).await?;
    reader.close();
    registry.shutdown()?;
    Ok(())
}

#[tokio::test]
async fn prepared_private_source_hands_off_real_pty_and_is_not_a_permission_snapshot() -> TestResult
{
    let temp = tempfile::tempdir()?;
    let path = shell(temp.path())?;
    let (registry, _documents, _owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let credit = reader.reserve()?;
    let (store, candidates) = reader
        .prepare(
            &registry,
            &scope,
            temp.path().join("state"),
            path,
            None,
            credit,
        )
        .await?;
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        registry.clone(),
        Handle::current(),
    )));
    let context = LocalSessionEnvironment {
        application_version: "test-prepared".into(),
        pane: None,
    };
    let spec = StoredProfileLaunch {
        cwd: None,
        candidates: &candidates,
        kind: ShellKind::Sh,
        context: &context,
        size: pulse_core::TermSize::new(32, 8)?,
        history: 16,
    };
    let id = timeout(
        Duration::from_secs(5),
        start_stored_page_local(&registry, &scope, &store, &root, spec),
    )
    .await??;
    let owner = root.lock().unwrap().take_for_finish(id)?;
    assert_eq!(
        timeout(Duration::from_secs(5), owner.finish())
            .await??
            .native
            .exit_code,
        0
    );
    assert_eq!(root.lock().unwrap().collect_reaped(), 1);
    // Reusing the same prepared store/candidates must reread the real deletion tombstone.
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await?;
    assert!(matches!(
        start_stored_page_local(&registry, &scope, &store, &root, spec).await,
        Err(LocalStartError::Profile(_))
    ));
    assert_eq!(root.lock().unwrap().occupied(), 0);
    reader.close();
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn visibility_edit_is_path_free_discovery_bound_and_survives_private_database_reopen()
-> TestResult {
    let temp = tempfile::tempdir()?;
    let path = shell(temp.path())?;
    let directory = temp.path().join("state");
    let (registry, documents, owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    let source = |kind| ProfileStart {
        cwd: None,
        preset: pulse_store::settings::TerminalPreset::Classic.into(),
        directory: directory.clone(),
        path: path.clone(),
        shell: None,
        kind,
    };
    reader
        .set_visibility(registry.clone(), scope.clone(), source(ShellKind::Sh), true)
        .await?;
    let offers = reader
        .list(
            registry.clone(),
            scope.clone(),
            directory.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert!(
        offers
            .iter()
            .any(|offer| offer.kind == "sh" && offer.hidden)
    );
    reader
        .set_visibility(
            registry.clone(),
            scope.clone(),
            source(ShellKind::Sh),
            false,
        )
        .await?;
    assert!(
        reader
            .list(
                registry.clone(),
                scope.clone(),
                directory.clone(),
                path.clone(),
                None
            )
            .await?
            .iter()
            .any(|offer| offer.kind == "sh" && !offer.hidden)
    );
    reader
        .set_visibility(registry.clone(), scope.clone(), source(ShellKind::Sh), true)
        .await?;
    let before = reader
        .store
        .get()
        .ok_or("store not created")?
        .store
        .load_auto_profiles()
        .await?;
    // System fallback directories are intentionally part of discovery; no particular
    // optional shell is assumed absent on another Unix developer/CI host.
    let offered = reader
        .list(
            registry.clone(),
            scope.clone(),
            directory.clone(),
            path.clone(),
            None,
        )
        .await?;
    if let Some(missing) = [
        ShellKind::Bash,
        ShellKind::Zsh,
        ShellKind::Fish,
        ShellKind::Sh,
        ShellKind::Pwsh,
    ]
    .into_iter()
    .find(|kind| {
        !offered
            .iter()
            .any(|offer| offer.kind == kind.executable_name())
    }) {
        assert!(
            reader
                .set_visibility(registry.clone(), scope.clone(), source(missing), true)
                .await
                .is_err()
        );
    }
    assert_eq!(
        reader
            .store
            .get()
            .ok_or("missing store")?
            .store
            .load_auto_profiles()
            .await?,
        before
    );
    documents.started(owner)?;
    assert_eq!(
        reader
            .set_visibility(registry.clone(), scope, source(ShellKind::Sh), false)
            .await,
        Err("workspace.denied")
    );
    assert_eq!(
        reader
            .store
            .get()
            .ok_or("missing store")?
            .store
            .load_auto_profiles()
            .await?,
        before
    );
    reader.shutdown().await?;
    let (store, task) = pulse_store::StateStore::open(directory.join("state.db")).await?;
    assert_eq!(store.load_auto_profiles().await?, before);
    task.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn visibility_edit_cannot_revive_deletion_or_substitute_another_source_directory()
-> TestResult {
    let temp = tempfile::tempdir()?;
    let path = shell(temp.path())?;
    let directory = temp.path().join("state");
    let (registry, _documents, _owner, scope) = page();
    let reader = Arc::new(ProfileReader::new());
    reader
        .list(
            registry.clone(),
            scope.clone(),
            directory.clone(),
            path.clone(),
            None,
        )
        .await?;
    let store = &reader.store.get().ok_or("missing store")?.store;
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await?;
    let expected = store.load_auto_profiles().await?;
    let source = |directory| ProfileStart {
        cwd: None,
        preset: pulse_store::settings::TerminalPreset::Classic.into(),
        directory,
        path: path.clone(),
        shell: None,
        kind: ShellKind::Sh,
    };
    assert!(
        reader
            .set_visibility(
                registry.clone(),
                scope.clone(),
                source(directory.clone()),
                false
            )
            .await
            .is_err()
    );
    assert_eq!(store.load_auto_profiles().await?, expected);
    assert_eq!(
        reader
            .set_visibility(
                registry.clone(),
                scope.clone(),
                source(temp.path().join("foreign")),
                false
            )
            .await,
        Err("workspace.denied")
    );
    assert_eq!(store.load_auto_profiles().await?, expected);
    assert!(!temp.path().join("foreign").exists());
    reader.shutdown().await?;
    assert_eq!(
        reader
            .set_visibility(registry.clone(), scope, source(directory), true)
            .await,
        Err("workspace.denied")
    );
    registry.shutdown()?;
    Ok(())
}
