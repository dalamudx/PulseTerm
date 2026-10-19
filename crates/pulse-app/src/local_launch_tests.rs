//! Trusted, fixed-program tests; do not expose the supplied program via IPC.
#![allow(clippy::unwrap_used)]
use super::*;
#[cfg(unix)]
use pulse_term::session::Lifecycle;
#[cfg(unix)]
use tokio::time::{Duration, timeout};

fn size() -> TermSize {
    TermSize::new(32, 8).unwrap()
}
fn command(body: &str) -> PtyCommand {
    let mut result = PtyCommand::new("/bin/sh");
    result.args = vec!["-c".into(), body.into()];
    result
}

#[test]
fn missing_runtime_cannot_reserve_or_spawn() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    assert!(matches!(
        start_local_registered(&registry, owner, command("read x"), size(), 16),
        Err(LocalStartError::Worker)
    ));
    assert!(registry.begin_local_launch(owner).is_ok());
}

#[cfg(unix)]
#[tokio::test]
async fn normal_handoff_survives_window_close_and_retains_true_native_exit() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let pending = start_local_registered(
        &registry,
        owner,
        command("printf 'READY\\n'; sleep 1; printf 'DONE\\n'"),
        size(),
        16,
    )
    .unwrap();
    let ready = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap()
        .unwrap();
    registry.close_window(owner).unwrap();
    let result = timeout(Duration::from_secs(5), ready.completion)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.actor.state.lifecycle, Lifecycle::Ended);
    assert!(result.actor.state.parsed_bytes >= "READY\r\nDONE\r\n".len() as u64);
    assert_eq!(result.native.exit_code, 0);
    registry.reap_closed(ready.id).unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn lost_receipt_after_registration_reaps_direct_child_and_session() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let pending = start_local_registered(&registry, owner, command("read x"), size(), 16).unwrap();
    let PendingLocalLaunch {
        receipt,
        accepted,
        worker,
    } = pending;
    let ready = timeout(Duration::from_secs(5), receipt)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        registry.reap_closed(ready.id),
        Err(RegistryError::Busy)
    ));
    // The caller received the registration but never accepted it. The worker,
    // rather than the dropped receipt, owns and joins the still-running child.
    let id = ready.id;
    drop(accepted);
    drop(ready);
    timeout(Duration::from_secs(5), worker)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(matches!(
        registry.reap_closed(id),
        Err(RegistryError::Forbidden)
    ));
}

#[cfg(unix)]
#[tokio::test]
async fn cancellation_waits_for_supervisor_without_leaking_launch_capacity() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let pending = start_local_registered(&registry, owner, command("read x"), size(), 16).unwrap();
    timeout(Duration::from_secs(5), pending.cancel().join())
        .await
        .unwrap()
        .unwrap();
    let next = registry.begin_local_launch(owner).unwrap();
    drop(next);
}
#[cfg(unix)]
#[tokio::test]
async fn registered_but_unaccepted_launch_is_reaped_without_waiting_for_caller() {
    let registry = SessionRegistry::new();
    let window = registry.open_window().unwrap();
    let PendingLocalLaunch {
        receipt,
        accepted,
        worker,
    } = start_local_registered(
        &registry,
        window,
        command("printf ready; read -r line"),
        TermSize::new(32, 8).unwrap(),
        16,
    )
    .unwrap();
    let registered = receipt.await.unwrap().unwrap();
    let id = registered.id();
    let mut root = crate::local_session_root::LocalSessionRoot::new(
        registry.clone(),
        tokio::runtime::Handle::current(),
    );
    timeout(Duration::from_secs(5), root.shutdown().join())
        .await
        .unwrap()
        .unwrap();
    worker.await.unwrap().unwrap();
    assert!(matches!(
        registry.reap_closed(id),
        Err(RegistryError::Forbidden)
    ));
    drop(accepted);
    drop(registered);
}
#[tokio::test]
async fn cancellation_receipt_propagates_cleanup_error_even_when_supervisor_task_returns_normally()
{
    let cancelled = CancelledLocalLaunch {
        worker: tokio::spawn(async { Err(LocalStartError::Worker) }),
    };
    assert!(matches!(
        cancelled.join().await,
        Err(LocalStartError::Worker)
    ));
}
