#![allow(clippy::unwrap_used)]
use super::*;
use crate::local_launch::start_local_registered;
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use tokio::time::{Duration, timeout};
fn command() -> PtyCommand {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), "printf ready; read -r line".into()];
    command
}
#[tokio::test]
async fn shutdown_closes_owned_and_already_borrowed_owners_until_native_reaped() {
    let registry = SessionRegistry::new();
    let window = registry.open_window().unwrap();
    let mut root = LocalSessionRoot::new(registry.clone(), Handle::current());
    let a = start_local_registered(
        &registry,
        window,
        command(),
        TermSize::new(32, 8).unwrap(),
        16,
    )
    .unwrap()
    .wait()
    .await
    .unwrap();
    let id = root.adopt(a).unwrap();
    let borrowed = root.take_for_finish(id).unwrap();
    let finishing = tokio::spawn(borrowed.finish());
    let b = start_local_registered(
        &registry,
        window,
        command(),
        TermSize::new(32, 8).unwrap(),
        16,
    )
    .unwrap()
    .wait()
    .await
    .unwrap();
    root.adopt(b).unwrap();
    timeout(Duration::from_secs(5), root.shutdown().join())
        .await
        .unwrap()
        .unwrap();
    finishing.await.unwrap().unwrap();
    assert_eq!(root.collect_reaped(), 2);
    assert_eq!(root.occupied(), 0);
    assert!(
        start_local_registered(
            &registry,
            window,
            command(),
            TermSize::new(32, 8).unwrap(),
            16
        )
        .is_err()
    );
}
#[tokio::test(flavor = "current_thread")]
async fn queued_launch_is_cancelled_by_seal_without_needing_page_to_receive_it() {
    let registry = SessionRegistry::new();
    let window = registry.open_window().unwrap();
    let pending = start_local_registered(
        &registry,
        window,
        command(),
        TermSize::new(32, 8).unwrap(),
        16,
    )
    .unwrap();
    let mut root = LocalSessionRoot::new(registry, Handle::current());
    timeout(Duration::from_secs(5), root.shutdown().join())
        .await
        .unwrap()
        .unwrap();
    assert!(pending.wait().await.is_err());
}
#[tokio::test]
async fn dropped_shutdown_waiter_never_aborts_cleanup_or_reopens_root() {
    let registry = SessionRegistry::new();
    let window = registry.open_window().unwrap();
    let registered = start_local_registered(
        &registry,
        window,
        command(),
        TermSize::new(32, 8).unwrap(),
        16,
    )
    .unwrap()
    .wait()
    .await
    .unwrap();
    let mut root = LocalSessionRoot::new(registry, Handle::current());
    root.adopt(registered).unwrap();
    drop(root.shutdown());
    timeout(Duration::from_secs(5), root.shutdown().join())
        .await
        .unwrap()
        .unwrap();
    root.collect_reaped();
    assert_eq!(root.occupied(), 0);
}
#[tokio::test]
async fn failed_or_disconnected_owner_proof_never_reports_success_or_releases_root_slot() {
    for state in [CleanupState::Failed, CleanupState::Closing] {
        let registry = SessionRegistry::new();
        let window = registry.open_window().unwrap();
        let (transport, _driver) = pulse_term::session::ChannelTransport::bounded_driver();
        let (handle, task) =
            pulse_term::session::spawn_session(TermSize::new(32, 8).unwrap(), 16, transport);
        let id = registry.register(window, handle.clone()).unwrap();
        let (sender, receiver) = watch::channel(state);
        drop(sender);
        let mut root = LocalSessionRoot::new(registry, Handle::current());
        root.retiring.insert(id, receiver);
        root.closing_handles.insert(id, handle);
        assert!(matches!(
            root.shutdown().join().await,
            Err(LocalStartError::Worker)
        ));
        root.collect_reaped();
        assert_eq!(root.occupied(), 1);
        task.join().await.unwrap();
    }
}
#[tokio::test]
async fn lost_started_native_supervisor_prevents_even_empty_root_from_claiming_drain() {
    let registry = SessionRegistry::new();
    let mut physical = registry.native_workers.reserve().unwrap();
    physical.start();
    drop(physical);
    let mut root = LocalSessionRoot::new(registry, Handle::current());
    assert!(matches!(
        root.shutdown().join().await,
        Err(LocalStartError::Worker)
    ));
}
