use super::*;
use profiles::{AutoProfileEdit as Edit, AutoProfilePreferences as Prefs, AutomaticShell as Shell};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
fn private_dir() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn hide() -> Edit {
    Edit::Visibility {
        shell: Shell::Bash,
        hidden: true,
    }
}
#[tokio::test]
async fn denied_and_panicking_predicates_cannot_mutate_or_close_the_shared_store()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let (store, task) = StateStore::open(dir.path().join("state.db")).await?;
    assert!(matches!(
        store.edit_auto_profiles_authorized(hide(), || false).await,
        Err(StoreError::ProfileAuthorization)
    ));
    assert_eq!(store.load_auto_profiles().await?, Prefs::default());
    assert!(matches!(
        store
            .edit_auto_profiles_authorized(hide(), || panic!("fixed guard fault"))
            .await,
        Err(StoreError::ProfileAuthorization)
    ));
    assert_eq!(store.load_auto_profiles().await?, Prefs::default());
    assert!(
        store
            .edit_auto_profiles_authorized(hide(), || true)
            .await?
            .hidden()
            .any(|s| s == Shell::Bash)
    );
    task.shutdown().await?;
    Ok(())
}
#[test]
fn retirement_before_actual_worker_dispatch_is_checked_not_enqueue_time()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("state.db"))?;
    let live = Arc::new(AtomicBool::new(true));
    let checks = Arc::new(AtomicUsize::new(0));
    let current = live.clone();
    let count = checks.clone();
    let (reply, mut receipt) = oneshot::channel();
    let request = Request::AuthorizedEdit(
        hide(),
        Arc::new(move || {
            count.fetch_add(1, Ordering::SeqCst);
            current.load(Ordering::SeqCst)
        }),
        reply,
    );
    live.store(false, Ordering::SeqCst);
    dispatch(&connection, request);
    assert!(matches!(
        receipt.try_recv()?,
        Err(StoreError::ProfileAuthorization)
    ));
    assert_eq!(checks.load(Ordering::SeqCst), 1);
    assert_eq!(database::load(&connection)?, Prefs::default());
    Ok(())
}
#[test]
fn dropped_receipt_before_dispatch_skips_authorization_and_write()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("state.db"))?;
    let checks = Arc::new(AtomicUsize::new(0));
    let observed = checks.clone();
    let (reply, receipt) = oneshot::channel();
    drop(receipt);
    dispatch(
        &connection,
        Request::AuthorizedEdit(
            hide(),
            Arc::new(move || {
                observed.fetch_add(1, Ordering::SeqCst);
                true
            }),
            reply,
        ),
    );
    assert_eq!(checks.load(Ordering::SeqCst), 0);
    assert_eq!(database::load(&connection)?, Prefs::default());
    Ok(())
}
struct Reservation(Arc<AtomicBool>);
impl Drop for Reservation {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
#[tokio::test]
async fn caller_abort_cannot_drop_worker_reservation_and_admitted_commit_survives_lost_receipt()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let path = dir.path().join("state.db");
    let (store, task) = StateStore::open(path.clone()).await?;
    let released = Arc::new(AtomicBool::new(false));
    let reservation = Reservation(released.clone());
    let (entered, entry) = oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered));
    let (exit, finish) = std::sync::mpsc::channel();
    let finish = std::sync::Mutex::new(finish);
    let handle = store.clone();
    let caller = tokio::spawn(async move {
        handle
            .edit_auto_profiles_authorized(hide(), move || {
                let _retain = &reservation;
                let Ok(mut start) = entered.lock() else {
                    return false;
                };
                if let Some(sender) = start.take() {
                    let _ = sender.send(());
                }
                drop(start);
                let Ok(receipt) = finish.lock() else {
                    return false;
                };
                receipt
                    .recv_timeout(std::time::Duration::from_secs(2))
                    .is_ok()
            })
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), entry).await??;
    caller.abort();
    assert!(caller.await.is_err());
    assert!(!released.load(Ordering::SeqCst));
    exit.send(())?;
    assert!(
        store
            .load_auto_profiles()
            .await?
            .hidden()
            .any(|s| s == Shell::Bash)
    );
    assert!(released.load(Ordering::SeqCst));
    task.shutdown().await?;
    let (store, task) = StateStore::open(path).await?;
    assert!(
        store
            .load_auto_profiles()
            .await?
            .hidden()
            .any(|s| s == Shell::Bash)
    );
    task.shutdown().await?;
    Ok(())
}
struct PublishOrder {
    receipt: Arc<std::sync::Mutex<oneshot::Receiver<Result<Prefs>>>>,
    released_before_publish: Arc<AtomicBool>,
}
impl Drop for PublishOrder {
    fn drop(&mut self) {
        if let Ok(mut receipt) = self.receipt.lock() {
            self.released_before_publish.store(
                matches!(receipt.try_recv(), Err(oneshot::error::TryRecvError::Empty)),
                Ordering::SeqCst,
            );
        }
    }
}
#[test]
fn native_operation_credit_is_released_before_success_receipt_can_be_consumed()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let dir = private_dir()?;
    let connection = database::open(&dir.path().join("state.db"))?;
    let (reply, receipt) = oneshot::channel();
    let receipt = Arc::new(std::sync::Mutex::new(receipt));
    let released_before_publish = Arc::new(AtomicBool::new(false));
    let order = PublishOrder {
        receipt: receipt.clone(),
        released_before_publish: released_before_publish.clone(),
    };
    dispatch(
        &connection,
        Request::AuthorizedEdit(
            hide(),
            Arc::new(move || {
                let _retain = &order;
                true
            }),
            reply,
        ),
    );
    assert!(released_before_publish.load(Ordering::SeqCst));
    let mut value = receipt.lock().map_err(|_| "fixture receipt lock")?;
    assert!(
        value
            .try_recv()??
            .hidden()
            .any(|shell| shell == Shell::Bash)
    );
    Ok(())
}
