use crate::{StateStore, ssh_hosts::HostEdit};
use pulse_core::ssh_endpoint::SshEndpoint;
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
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
#[tokio::test]
async fn aborted_waiter_and_worker_shutdown_keep_credit_until_actual_transaction_exit() -> TestResult
{
    cancelled_native_work(false).await
}
#[tokio::test]
async fn cancelled_owned_read_keeps_credit_until_actual_query_and_worker_exit() -> TestResult {
    cancelled_native_work(true).await
}
async fn cancelled_native_work(read_only: bool) -> TestResult {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = d.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let snapshot = store.load_ssh_hosts().await?;
    let (entered, started) = tokio::sync::oneshot::channel();
    let notify = Mutex::new(Some(entered));
    let calls = AtomicUsize::new(0);
    let credit_alive = Arc::new(AtomicBool::new(true));
    let credit = Credit(credit_alive.clone());
    let gate = Arc::new((Mutex::new(false), Condvar::new()));
    let blocked = gate.clone();
    let authorize = Arc::new(move || {
        credit.keep();
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(sender) = notify.lock().ok().and_then(|mut s| s.take()) {
                let _ = sender.send(());
            }
            let (lock, ready) = &*blocked;
            let Ok(mut released) = lock.lock() else {
                return false;
            };
            while !*released {
                let Ok(next) = ready.wait(released) else {
                    return false;
                };
                released = next;
            }
        }
        true
    });
    let endpoint = SshEndpoint::new("隔离主机", "example.com", 22, "user")?;
    let client = store.clone();
    let waiter = tokio::spawn(async move {
        if read_only {
            client.load_ssh_hosts_authorized(authorize).await
        } else {
            client
                .edit_ssh_hosts(snapshot, HostEdit::Create(endpoint), authorize)
                .await
        }
    });
    started.await?;
    waiter.abort();
    let result = waiter.await;
    assert!(result.is_err());
    assert!(credit_alive.load(Ordering::SeqCst));
    worker.cancel();
    let drain = tokio::spawn(worker.join());
    tokio::task::yield_now().await;
    assert!(!drain.is_finished());
    assert!(credit_alive.load(Ordering::SeqCst));
    let (lock, ready) = &*gate;
    *lock.lock().map_err(|_| "fixture poisoned")? = true;
    ready.notify_all();
    drain.await??;
    assert!(!credit_alive.load(Ordering::SeqCst));
    let (reopened, new_worker) = StateStore::open(path).await?;
    let after = reopened.load_ssh_hosts().await?;
    assert_eq!(after.revision(), 0);
    assert!(after.records().is_empty());
    drop(reopened);
    new_worker.join().await?;
    Ok(())
}
