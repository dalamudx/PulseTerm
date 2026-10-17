use super::super::{Shared, State as PeerState, revocable_io::RevocableIo};
use super::*;
use std::sync::{
    Weak,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Duration;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn shared() -> Arc<Shared> {
    Arc::new(Shared {
        io: IoLifetime::new(),
        wakeups: Mutex::new([None, None]),
        closed: AtomicBool::new(false),
        state: Mutex::new(PeerState {
            claim: None,
            refusal: None,
        }),
    })
}
struct BlockingDrop {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
    done: Arc<AtomicBool>,
}
impl Drop for BlockingDrop {
    fn drop(&mut self) {
        let _sent = self.entered.send(());
        let released = self.release.recv_timeout(Duration::from_secs(3)).is_ok();
        self.done.store(released, Ordering::Release);
    }
}
struct Admission {
    shared: Weak<Shared>,
    done: Arc<AtomicBool>,
    outside: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
}
impl Drop for Admission {
    fn drop(&mut self) {
        let unlocked = self.shared.upgrade().is_some_and(|s| {
            s.io.state.try_lock().is_ok()
                && s.state.try_lock().is_ok()
                && s.wakeups.try_lock().is_ok()
        });
        self.outside.store(
            unlocked && self.done.load(Ordering::Acquire),
            Ordering::Release,
        );
        self.drops.fetch_add(1, Ordering::SeqCst);
    }
}
struct Fixture {
    state: Arc<Shared>,
    stream: RevocableIo<BlockingDrop>,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
    admission: Weak<Admission>,
    outside: Arc<AtomicBool>,
    drops: Arc<AtomicUsize>,
}
fn fixture() -> Result<Fixture, PeerError> {
    let state = shared();
    let (tx, entered) = mpsc::channel();
    let (release, rx) = mpsc::channel();
    let done = Arc::new(AtomicBool::new(false));
    let outside = Arc::new(AtomicBool::new(false));
    let drops = Arc::new(AtomicUsize::new(0));
    let value = Arc::new(Admission {
        shared: Arc::downgrade(&state),
        done: done.clone(),
        outside: outside.clone(),
        drops: drops.clone(),
    });
    let admission = Arc::downgrade(&value);
    drop(state.io.retain(value)?);
    let stream = RevocableIo::new(
        BlockingDrop {
            entered: tx,
            release: rx,
            done,
        },
        state.clone(),
    );
    Ok(Fixture {
        state,
        stream,
        entered,
        release,
        admission,
        outside,
        drops,
    })
}
#[test]
fn closing_evidence_and_dropped_waiter_do_not_release_admission_before_inner_drop_finishes()
-> TestResult {
    let f = fixture()?;
    let completion = f.state.io.receipt();
    f.state.close();
    assert!(!completion.is_complete());
    assert!(f.admission.upgrade().is_some());
    assert!(matches!(
        f.state.io.retain(Arc::new(())),
        Err(PeerError::AdmissionBusy)
    ));
    drop(completion);
    let stream = f.stream;
    let task = std::thread::spawn(move || drop(stream));
    f.entered.recv_timeout(Duration::from_secs(2))?;
    let completion = f.state.io.receipt();
    assert!(!completion.is_complete());
    assert!(f.admission.upgrade().is_some());
    assert_eq!(f.drops.load(Ordering::SeqCst), 0);
    assert!(matches!(
        f.state.io.retain(Arc::new(())),
        Err(PeerError::Stale)
    ));
    f.release.send(())?;
    task.join().map_err(|_| "fixture.drop_panic")?;
    assert!(completion.is_complete());
    assert!(f.admission.upgrade().is_none());
    assert!(f.outside.load(Ordering::Acquire));
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn runtime_shutdown_retains_actual_stream_reservation_not_only_future_lifetime() -> TestResult {
    let f = fixture()?;
    let completion = f.state.io.receipt();
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(1)
        .enable_all()
        .build()?;
    let (tx, started) = mpsc::channel();
    let stream = f.stream;
    let task = runtime.spawn(async move {
        let _sent = tx.send(());
        std::future::pending::<()>().await;
        drop(stream);
    });
    started.recv_timeout(Duration::from_secs(2))?;
    let stop = std::thread::spawn(move || drop(runtime));
    f.entered.recv_timeout(Duration::from_secs(2))?;
    assert!(f.admission.upgrade().is_some());
    assert!(!completion.is_complete());
    assert_eq!(f.drops.load(Ordering::SeqCst), 0);
    drop(task);
    f.release.send(())?;
    stop.join().map_err(|_| "fixture.shutdown_panic")?;
    assert!(completion.is_complete());
    assert!(f.admission.upgrade().is_none());
    assert!(f.outside.load(Ordering::Acquire));
    assert_eq!(f.drops.load(Ordering::SeqCst), 1);
    Ok(())
}
#[tokio::test]
async fn orphaned_completion_is_error_and_observer_never_keeps_producer_or_reservation()
-> TestResult {
    let io = IoLifetime::new();
    let receipt = io.receipt();
    drop(io);
    assert!(matches!(receipt.wait().await, Err(PeerError::Transport)));
    let io = IoLifetime::new();
    let reservation = Arc::new(());
    let weak = Arc::downgrade(&reservation);
    let receipt = io.retain(reservation)?;
    let held = io.begin_drop();
    assert!(weak.upgrade().is_some());
    drop(held);
    io.finished();
    receipt.wait().await?;
    assert!(weak.upgrade().is_none());
    Ok(())
}
