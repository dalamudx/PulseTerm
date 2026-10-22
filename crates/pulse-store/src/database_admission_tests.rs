use super::*;
use crate::{StateStore, StoreError};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::Notify;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

struct Guard {
    valid: Arc<AtomicBool>,
    dropped: Arc<AtomicBool>,
    done: Arc<Notify>,
}
impl DatabaseAdmission for Guard {
    fn validate(&self) -> Result<()> {
        if self.valid.load(Ordering::SeqCst) {
            Ok(())
        } else {
            Err(StoreError::Location)
        }
    }
    fn backup(&self, _: u32) -> Result<Box<dyn DatabaseBackup + '_>> {
        Err(StoreError::Backup)
    }
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
        self.done.notify_one();
    }
}
fn guard() -> Guard {
    Guard {
        valid: Arc::new(AtomicBool::new(true)),
        dropped: Arc::new(AtomicBool::new(false)),
        done: Arc::new(Notify::new()),
    }
}
fn root() -> std::io::Result<tempfile::TempDir> {
    let root = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(root)
}

#[tokio::test]
async fn rejection_runs_off_runtime_and_never_creates_a_database() -> TestResult {
    let root = root()?;
    let path = root.path().join("state.db");
    let caller = std::thread::current().id();
    let result = StateStore::open_admitted(
        path.clone(),
        move |_| {
            assert_ne!(std::thread::current().id(), caller);
            Err(StoreError::Location)
        },
        (),
    )
    .await;
    assert!(matches!(result, Err(StoreError::Location)));
    assert!(!path.exists());
    Ok(())
}

#[tokio::test]
async fn admission_lives_until_real_shutdown_and_handles_cannot_keep_it_alive() -> TestResult {
    let root = root()?;
    let initialization = guard();
    let initialization_dropped = initialization.dropped.clone();
    let guard = guard();
    let dropped = guard.dropped.clone();
    let (store, task) = StateStore::open_admitted(
        root.path().join("state.db"),
        move |_| Ok(Box::new(guard)),
        initialization,
    )
    .await?;
    store.load_auto_profiles().await?;
    assert!(initialization_dropped.load(Ordering::SeqCst));
    assert!(!dropped.load(Ordering::SeqCst));
    task.shutdown().await?;
    assert!(dropped.load(Ordering::SeqCst));
    assert!(matches!(
        store.load_auto_profiles().await,
        Err(StoreError::Closed)
    ));
    Ok(())
}

#[tokio::test]
async fn revoked_file_admission_stops_dispatch_and_still_closes_the_worker() -> TestResult {
    let root = root()?;
    let guard = guard();
    let valid = guard.valid.clone();
    let dropped = guard.dropped.clone();
    let (store, task) = StateStore::open_admitted(
        root.path().join("state.db"),
        move |_| Ok(Box::new(guard)),
        (),
    )
    .await?;
    valid.store(false, Ordering::SeqCst);
    assert!(matches!(
        store.load_auto_profiles().await,
        Err(StoreError::Closed)
    ));
    assert_eq!(task.shutdown().await, Err(StoreError::Location));
    assert!(dropped.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn cancelled_waiter_cannot_release_admission_while_native_initialization_is_active()
-> TestResult {
    let root = root()?;
    let path = root.path().join("state.db");
    let initialization = guard();
    let initialization_dropped = initialization.dropped.clone();
    let guard = guard();
    let dropped = guard.dropped.clone();
    let done = guard.done.clone();
    let (started, ready) = tokio::sync::oneshot::channel();
    let (release, blocked) = std::sync::mpsc::channel();
    let waiting = tokio::spawn(StateStore::open_admitted(
        path,
        move |_| {
            let _ = started.send(());
            blocked
                .recv_timeout(std::time::Duration::from_secs(5))
                .map_err(|_| StoreError::Closed)?;
            Ok(Box::new(guard) as Box<dyn DatabaseAdmission>)
        },
        initialization,
    ));
    ready.await?;
    waiting.abort();
    let _ = waiting.await;
    assert!(!dropped.load(Ordering::SeqCst));
    assert!(!initialization_dropped.load(Ordering::SeqCst));
    release.send(())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), done.notified()).await?;
    assert!(dropped.load(Ordering::SeqCst));
    Ok(())
}

#[tokio::test]
async fn refused_native_backup_preserves_the_old_schema_and_creates_no_backup() -> TestResult {
    let root = root()?;
    let path = root.path().join("state.db");
    crate::database::open_v1_fixture(&path)?
        .close()
        .map_err(|_| StoreError::Database)?;
    let guard = guard();
    let result = StateStore::open_admitted(path.clone(), move |_| Ok(Box::new(guard)), ()).await;
    assert!(matches!(result, Err(StoreError::Backup)));
    assert!(!root.path().join("state.db.bak-5").exists());
    let original = crate::database::open_v1_fixture(&path)?;
    assert_eq!(
        original.pragma_query_value(None, "user_version", |row| row.get::<_, u32>(0))?,
        1
    );
    original.close().map_err(|_| StoreError::Database)?;
    Ok(())
}

#[tokio::test]
async fn cancellation_after_native_ready_keeps_credit_through_connection_and_admission_close()
-> TestResult {
    struct ReadyWake(std::sync::mpsc::Sender<()>);
    impl std::task::Wake for ReadyWake {
        fn wake(self: Arc<Self>) {
            let _ = self.0.send(());
        }
    }
    struct Credit {
        admission_dropped: Arc<AtomicBool>,
        released_after_close: Arc<AtomicBool>,
        done: Arc<Notify>,
    }
    impl Drop for Credit {
        fn drop(&mut self) {
            self.released_after_close.store(
                self.admission_dropped.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            self.done.notify_one();
        }
    }
    let root = root()?;
    let guard = guard();
    let released_after_close = Arc::new(AtomicBool::new(false));
    let done = Arc::new(Notify::new());
    let credit = Credit {
        admission_dropped: guard.dropped.clone(),
        released_after_close: released_after_close.clone(),
        done: done.clone(),
    };
    let mut opening = Box::pin(StateStore::open_admitted(
        root.path().join("state.db"),
        move |_| Ok(Box::new(guard)),
        credit,
    ));
    let (ready, wake) = std::sync::mpsc::channel();
    let waker = std::task::Waker::from(Arc::new(ReadyWake(ready)));
    let mut context = std::task::Context::from_waker(&waker);
    assert!(std::future::Future::poll(opening.as_mut(), &mut context).is_pending());
    // Observe native readiness without ever polling/accepting its ownership handoff.
    wake.recv_timeout(std::time::Duration::from_secs(5))?;
    drop(opening);
    tokio::time::timeout(std::time::Duration::from_secs(5), done.notified()).await?;
    assert!(released_after_close.load(Ordering::SeqCst));
    Ok(())
}
