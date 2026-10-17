use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Source = (PathBuf, OsString, Option<PathBuf>);
type PageFixture = (
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
);
fn page() -> std::result::Result<PageFixture, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let scope = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, scope))
}
fn fixture() -> std::io::Result<(tempfile::TempDir, Source)> {
    let root = tempfile::tempdir()?;
    let bin = root.path().join("bin");
    fs::create_dir(&bin)?;
    for name in ["sh", "bash"] {
        let path = bin.join(name);
        fs::write(&path, b"not executed")?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    }
    let source = (root.path().join("state"), bin.into_os_string(), None);
    Ok((root, source))
}

#[tokio::test]
async fn lifecycle_fixed_target_single_use_and_separate_default_authority() -> TestResult {
    let (_root, source) = fixture()?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let default = reader
        .default_read(
            registry.clone(),
            page.clone(),
            source.0.clone(),
            source.1.clone(),
            None,
        )
        .await?;
    let wire = serde_json::to_value(default)?;
    let default_id = Uuid::parse_str(wire["ticket"].as_str().ok_or("ticket")?)?;
    reader
        .default_save(
            registry.clone(),
            page.clone(),
            source.clone(),
            default_id,
            Some(AutomaticShell::Sh),
        )
        .await?;
    let default = reader
        .default_read(
            registry.clone(),
            page.clone(),
            source.0.clone(),
            source.1.clone(),
            None,
        )
        .await?;
    let wire = serde_json::to_value(default)?;
    let default_id = Uuid::parse_str(wire["ticket"].as_str().ok_or("ticket")?)?;
    let offer = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    assert!(offer.is_default);
    assert_eq!(offer.shell, AutomaticShell::Sh);
    assert_eq!(offer.action, "delete");
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), default_id)
            .await,
        Err("workspace.denied")
    );
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                source.clone(),
                offer.ticket,
                None
            )
            .await,
        Err("workspace.denied")
    );
    reader
        .lifecycle_commit(registry.clone(), page.clone(), offer.ticket)
        .await?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), offer.ticket)
            .await,
        Err("workspace.denied")
    );
    assert_eq!(
        reader
            .lifecycle_deleted(registry.clone(), page.clone(), source.clone())
            .await?,
        [AutomaticShell::Sh]
    );
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                source.clone(),
                default_id,
                None
            )
            .await,
        Err("workspace.profiles_conflict")
    );
    let restore = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Restore(AutomaticShell::Sh),
        )
        .await?;
    assert_eq!(restore.action, "restore");
    assert!(restore.is_default);
    reader
        .lifecycle_commit(registry.clone(), page.clone(), restore.ticket)
        .await?;
    assert!(
        reader
            .lifecycle_deleted(registry.clone(), page.clone(), source.clone())
            .await?
            .is_empty()
    );
    let value = reader
        .default_read(
            registry.clone(),
            page.clone(),
            source.0.clone(),
            source.1.clone(),
            None,
        )
        .await?;
    let wire = serde_json::to_value(value)?;
    assert_eq!(wire["selected"], "sh");
    assert_eq!(wire["available"], true);
    reader.shutdown().await?;
    assert!(!source.0.join("native.pid").exists());
    Ok(())
}

#[tokio::test]
async fn lifecycle_cancel_replacement_expiry_navigation_and_wrong_action_reject() -> TestResult {
    let (_root, source) = fixture()?;
    let (registry, documents, owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    assert!(
        reader
            .lifecycle_prepare(
                registry.clone(),
                page.clone(),
                source.clone(),
                ProfileLifecycleEdit::Restore(AutomaticShell::Sh)
            )
            .await
            .is_err()
    );
    let canceled = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    reader.lifecycle_cancel(&registry, &page, canceled.ticket)?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), canceled.ticket)
            .await,
        Err("workspace.denied")
    );
    let old = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    let expired = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Bash),
        )
        .await?;
    reader.lifecycle_cancel(&registry, &page, old.ticket)?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), old.ticket)
            .await,
        Err("workspace.denied")
    );
    {
        let mut bank = reader.lifecycle_ticket.lock().map_err(|_| "poisoned")?;
        bank.as_mut().ok_or("ticket")?.deadline = Instant::now() - Duration::from_secs(1);
    }
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), expired.ticket)
            .await,
        Err("workspace.denied")
    );
    let stale = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    documents.started(owner)?;
    reader.retire_read_waiters();
    assert!(
        reader
            .lifecycle_ticket
            .lock()
            .map_err(|_| "poisoned")?
            .is_none()
    );
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let fresh = documents.current(owner, nonce)?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), fresh.clone(), stale.ticket)
            .await,
        Err("workspace.denied")
    );
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page, stale.ticket)
            .await,
        Err("workspace.denied")
    );
    assert!(
        reader
            .lifecycle_deleted(registry, fresh, source)
            .await?
            .is_empty()
    );
    reader.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn lifecycle_external_source_change_rejects_old_preparation() -> TestResult {
    let (_root, source) = fixture()?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    let (external, task) = StateStore::open(source.0.join("state.db")).await?;
    let snapshot = external.load_default_profile().await?;
    external
        .save_default_profile(snapshot, Some(AutomaticShell::Bash), || true)
        .await?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), offer.ticket)
            .await,
        Err("workspace.profiles_conflict")
    );
    assert!(
        reader
            .lifecycle_deleted(registry.clone(), page.clone(), source.clone())
            .await?
            .is_empty()
    );
    let offer = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    external
        .edit_auto_profiles(pulse_store::profiles::AutoProfileEdit::Visibility {
            shell: AutomaticShell::Sh,
            hidden: true,
        })
        .await?;
    assert_eq!(
        reader
            .lifecycle_commit(registry.clone(), page.clone(), offer.ticket)
            .await,
        Err("workspace.profiles_conflict")
    );
    let snapshot = external.load_default_profile().await?;
    assert_eq!(snapshot.shell(), Some(AutomaticShell::Bash));
    assert!(snapshot.preferences().deleted().next().is_none());
    reader.shutdown().await?;
    drop(external);
    task.join().await?;
    Ok(())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn lifecycle_cancel_queued_commit_keeps_credit_and_exit_barrier_until_worker_exits()
-> TestResult {
    use std::sync::{Condvar, Mutex, atomic::AtomicUsize};
    struct Release(Arc<(Mutex<bool>, Condvar)>);
    impl Drop for Release {
        fn drop(&mut self) {
            if let Ok(mut released) = self.0.0.lock() {
                *released = true;
                self.0.1.notify_all();
            }
        }
    }
    let (_root, source) = fixture()?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .lifecycle_prepare(
            registry.clone(),
            page.clone(),
            source.clone(),
            ProfileLifecycleEdit::Delete(AutomaticShell::Sh),
        )
        .await?;
    let store = reader
        .lifecycle_ticket
        .lock()
        .map_err(|_| "poisoned")?
        .as_ref()
        .ok_or("ticket")?
        .store
        .clone();
    let snapshot = store.load_default_profile().await?;
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let (entered, entry) = std::sync::mpsc::sync_channel(1);
    let calls = AtomicUsize::new(0);
    let blocker = tokio::spawn(async move {
        store
            .save_default_profile(snapshot, None, move || {
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    if entered.send(()).is_err() {
                        return false;
                    }
                    let Ok(released) = gate.0.lock() else {
                        return false;
                    };
                    return gate
                        .1
                        .wait_timeout_while(released, Duration::from_secs(5), |released| !*released)
                        .is_ok_and(|(released, _)| *released);
                }
                true
            })
            .await
    });
    entry.recv_timeout(Duration::from_secs(3))?;
    let pending = {
        let reader = reader.clone();
        let registry = registry.clone();
        let page = page.clone();
        tokio::spawn(async move { reader.lifecycle_commit(registry, page, offer.ticket).await })
    };
    tokio::time::timeout(Duration::from_secs(3), async {
        while reader.credit.available_permits() != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    reader.lifecycle_cancel(&registry, &page, offer.ticket)?;
    pending.abort();
    let _ = pending.await;
    assert_eq!(reader.credit.available_permits(), 0);
    let shutting = {
        let reader = reader.clone();
        tokio::spawn(async move { reader.shutdown().await })
    };
    tokio::task::yield_now().await;
    assert!(!shutting.is_finished());
    assert_eq!(reader.credit.available_permits(), 0);
    blocker.abort();
    let _ = blocker.await;
    drop(release);
    tokio::time::timeout(Duration::from_secs(3), shutting).await???;
    let (store, task) = StateStore::open(source.0.join("state.db")).await?;
    assert!(
        store
            .load_default_profile()
            .await?
            .preferences()
            .deleted()
            .next()
            .is_none()
    );
    drop(store);
    task.join().await?;
    Ok(())
}
