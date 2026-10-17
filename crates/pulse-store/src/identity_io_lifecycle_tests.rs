use crate::{
    StateStore, ssh_identities::IdentityEdit, vault_database::VaultEdit,
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use std::sync::{
    Arc, Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Credit(Arc<AtomicBool>);
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
async fn identity_read_cancel_shutdown_keeps_native_credit() -> TestResult {
    cancel(true).await
}
#[tokio::test]
async fn identity_write_cancel_shutdown_keeps_native_credit_and_does_not_commit() -> TestResult {
    cancel(false).await
}
async fn cancel(read: bool) -> TestResult {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = d.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    store
        .edit_vault_ciphertext(
            empty,
            VaultEdit::Initialize(
                vault,
                ValidationWrappedDek::decode_ciphertext(&bytes, vault)?,
            ),
            Arc::new(|| true),
        )
        .await?;
    let expected = store.load_ssh_identities(Arc::new(|| true)).await?;
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let calls = AtomicUsize::new(0);
    let alive = Arc::new(AtomicBool::new(true));
    let credit = Credit(alive.clone());
    let blocked = Arc::new((Mutex::new(false), Condvar::new()));
    let release = Release(blocked.clone());
    let authorize = Arc::new(move || {
        assert!(credit.0.load(Ordering::SeqCst));
        if calls.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(sender) = entered.lock().ok().and_then(|mut value| value.take()) {
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
    let identity = SshPasswordIdentity::new(
        "cancel-fixture",
        "fixed-user",
        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, true)?,
    )?;
    let client = store.clone();
    let request = tokio::spawn(async move {
        if read {
            client.load_ssh_identities(authorize).await
        } else {
            client
                .edit_ssh_identities(expected, IdentityEdit::Create(identity), authorize)
                .await
        }
    });
    started.await?;
    request.abort();
    assert!(matches!(request.await,Err(error) if error.is_cancelled()));
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
    let (store, worker) = StateStore::open(path).await?;
    assert_eq!(
        store
            .load_ssh_identities(Arc::new(|| true))
            .await?
            .revision(),
        0
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
