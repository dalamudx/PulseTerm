use crate::{StateStore, vault_database::VaultEdit, vault_wrap::ValidationWrappedDek};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Credit(Arc<AtomicBool>);
impl Credit {
    fn keep(&self) {
        assert!(self.0.load(Ordering::SeqCst));
    }
}
impl Drop for Credit {
    fn drop(&mut self) {
        self.0.store(false, Ordering::SeqCst);
    }
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        let (lock, ready) = &*self.0;
        *lock.lock().unwrap_or_else(|p| p.into_inner()) = true;
        ready.notify_all();
    }
}
#[tokio::test]
async fn read_waiter_cancel_and_shutdown_retain_credit_until_native_exit() -> TestResult {
    cancel(true).await
}
#[tokio::test]
async fn write_waiter_cancel_and_shutdown_retain_credit_without_commit() -> TestResult {
    cancel(false).await
}
async fn cancel(read_only: bool) -> TestResult {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = d.path().join("state.sqlite3");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let snapshot = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    let (entered, started) = tokio::sync::oneshot::channel();
    let notify = Mutex::new(Some(entered));
    let calls = AtomicUsize::new(0);
    let alive = Arc::new(AtomicBool::new(true));
    let credit = Credit(alive.clone());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(gate.clone());
    let blocked = gate.clone();
    let authorize = Arc::new(move || {
        credit.keep();
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(sender) = notify.lock().ok().and_then(|mut s| s.take()) {
                let _ = sender.send(());
            }
            let (lock, ready) = &*blocked;
            let Ok(mut flag) = lock.lock() else {
                return false;
            };
            while !*flag {
                let Ok(next) = ready.wait(flag) else {
                    return false;
                };
                flag = next;
            }
        }
        true
    });
    let id = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    let envelope = ValidationWrappedDek::decode_ciphertext(&bytes, id)?;
    let client = store.clone();
    let waiter = tokio::spawn(async move {
        if read_only {
            client.load_vault_ciphertext(authorize).await
        } else {
            client
                .edit_vault_ciphertext(snapshot, VaultEdit::Initialize(id, envelope), authorize)
                .await
        }
    });
    started.await?;
    waiter.abort();
    assert!(waiter.await.is_err());
    assert!(alive.load(Ordering::SeqCst));
    worker.cancel();
    let drain = tokio::spawn(worker.join());
    tokio::task::yield_now().await;
    assert!(!drain.is_finished());
    assert!(alive.load(Ordering::SeqCst));
    drop(release);
    drain.await??;
    assert!(!alive.load(Ordering::SeqCst));
    drop(store);
    let (reopened, worker) = StateStore::open(path).await?;
    assert_eq!(
        reopened
            .load_vault_ciphertext(Arc::new(|| true))
            .await?
            .revision(),
        0
    );
    drop(reopened);
    worker.join().await?;
    Ok(())
}
