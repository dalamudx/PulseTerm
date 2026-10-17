//! The production worker itself owns both CPU admission and the native guard through actual exit.
use super::{DECODE_BUDGET, Guard, VaultOwnerError, spawn_decode};
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use zeroize::Zeroizing;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Release(Option<mpsc::Sender<()>>);
impl Release {
    fn finish(&mut self) -> TestResult {
        if let Some(sender) = self.0.take() {
            sender.send(())?;
        }
        Ok(())
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        if let Some(sender) = self.0.take() {
            let _ = sender.send(());
        }
    }
}
fn decode_lifecycle(shutdown: bool) -> TestResult {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(1)
        .build()?;
    let (entered_tx, entered_rx) = mpsc::channel();
    let (finish_tx, finish_rx) = mpsc::channel();
    // Release is dropped before Runtime on every error, so a failed assertion cannot strand a thread.
    let mut release = Release(Some(finish_tx));
    let cancelled = Arc::new(AtomicBool::new(false));
    let cancellation = cancelled.clone();
    let once = AtomicBool::new(false);
    let receiver = Mutex::new(finish_rx);
    let guard: Guard = Arc::new(move || {
        if !once.swap(true, Ordering::AcqRel) {
            if entered_tx.send(()).is_err() {
                return false;
            }
            let Ok(receiver) = receiver.lock() else {
                return false;
            };
            if receiver.recv().is_err() {
                return false;
            }
        }
        !cancellation.load(Ordering::Acquire)
    });
    let witness = Arc::downgrade(&guard);
    let job = runtime.block_on(async {
        let permit = DECODE_BUDGET.clone().acquire_owned().await?;
        Ok::<_, Box<dyn std::error::Error>>(spawn_decode(
            Zeroizing::new(b"fixed-cancelled-input".to_vec()),
            None,
            permit,
            guard,
        ))
    })?;
    entered_rx.recv_timeout(std::time::Duration::from_secs(2))?;
    cancelled.store(true, Ordering::Release);
    job.abort();
    let mut runtime = Some(runtime);
    if shutdown && let Some(current) = runtime.take() {
        current.shutdown_background();
    }
    let occupied = DECODE_BUDGET.clone().try_acquire_owned().is_err();
    let retained = witness.upgrade().is_some();
    release.finish()?;
    let observer = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let result = observer
        .block_on(async { tokio::time::timeout(std::time::Duration::from_secs(2), job).await });
    let denied = matches!(result?, Ok(Err(VaultOwnerError::Denied)));
    let permit = observer.block_on(async {
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            DECODE_BUDGET.clone().acquire_owned(),
        )
        .await
    })??;
    let cleared = witness.upgrade().is_none();
    drop(permit);
    drop(runtime);
    if !(occupied && retained && denied && cleared) {
        return Err("private_key.decode_lifecycle".into());
    }
    Ok(())
}
#[test]
fn private_key_decode_cancel_keeps_cpu_and_native_guard_until_worker_exit() -> TestResult {
    decode_lifecycle(false)
}
#[test]
fn private_key_decode_runtime_shutdown_keeps_cpu_and_native_guard_until_worker_exit() -> TestResult
{
    decode_lifecycle(true)
}
