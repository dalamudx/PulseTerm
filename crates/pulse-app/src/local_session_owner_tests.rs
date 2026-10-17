//! Fixed Unix children only; no user shell files, executable IPC or timer-based polling.
#![allow(clippy::unwrap_used)]
use super::*;
use crate::{
    local_launch::start_local_registered,
    registry::{RegistryError, WindowToken},
};
use pulse_core::TermSize;
use pulse_pty::PtyCommand;
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

async fn launch(
    registry: &SessionRegistry,
    owner: WindowToken,
    body: &str,
) -> RegisteredLocalSession {
    let mut command = PtyCommand::new("/bin/sh");
    command.args = vec!["-c".into(), body.into()];
    timeout(
        Duration::from_secs(5),
        start_local_registered(registry, owner, command, TermSize::new(32, 8).unwrap(), 16)
            .unwrap()
            .wait(),
    )
    .await
    .unwrap()
    .unwrap()
}
async fn running(handle: &SessionHandle) {
    let mut state = handle.state.clone();
    timeout(Duration::from_secs(5), async {
        while state.borrow().parsed_bytes == 0 {
            state.changed().await.unwrap();
        }
    })
    .await
    .unwrap();
    assert_eq!(state.borrow().lifecycle, Lifecycle::Running);
}
async fn reaped(observer: &mut watch::Receiver<CleanupState>) {
    timeout(
        Duration::from_secs(5),
        observer.wait_for(|state| *state == CleanupState::Reaped),
    )
    .await
    .unwrap()
    .unwrap();
}
fn absent(registry: &SessionRegistry, owner: WindowToken, id: SessionId) {
    assert!(matches!(
        registry.close_session(owner, id),
        Err(RegistryError::Forbidden)
    ));
}

#[tokio::test]
async fn natural_exit_preserves_receipt_and_retires_registered_slot() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let ready = launch(&registry, owner, "printf done; exit 7").await;
    let id = ready.id;
    let owned = LocalSessionOwner::adopt(Handle::current(), ready);
    assert_eq!(owned.id(), id);
    let mut observer = owned.observe_cleanup();
    let receipt = timeout(Duration::from_secs(5), owned.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.native.exit_code, 7);
    assert!(!receipt.native.cancelled);
    assert_eq!(receipt.actor.state.lifecycle, Lifecycle::Ended);
    assert_eq!(receipt.actor.state.parsed_bytes, 4);
    reaped(&mut observer).await;
    absent(&registry, owner, id);
    registry.shutdown().unwrap();
}

#[tokio::test]
async fn document_and_window_retirement_do_not_drop_root_owner() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let _old = registry.activate_launch_page(owner).unwrap();
    let ready = launch(&registry, owner, "printf ready; read -r line").await;
    let owned = LocalSessionOwner::adopt(Handle::current(), ready);
    running(owned.handle()).await;
    let _new = registry.activate_launch_page(owner).unwrap();
    registry.close_window(owner).unwrap();
    assert_eq!(owned.handle().state.borrow().lifecycle, Lifecycle::Running);
    let mut observer = owned.observe_cleanup();
    assert_eq!(*observer.borrow(), CleanupState::Owned);
    let receipt = timeout(Duration::from_secs(5), owned.cancel().join())
        .await
        .unwrap()
        .unwrap();
    // PTY hangup may reap the child before the controller sends a kill; native
    // cancelled is that controller observation, not a synonym for close requested.
    assert!(matches!(
        receipt.actor.state.lifecycle,
        Lifecycle::Closed | Lifecycle::Ended
    ));
    reaped(&mut observer).await;
    registry.shutdown().unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn cancelled_finish_keeps_receiver_for_detached_drop_cleanup() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let ready = launch(&registry, owner, "printf ready; read -r line").await;
    let id = ready.id;
    let owned = LocalSessionOwner::adopt(Handle::current(), ready);
    running(owned.handle()).await;
    let mut observer = owned.observe_cleanup();
    let mut finish = Box::pin(owned.finish());
    tokio::select! { biased; _ = &mut finish => panic!("running session finished early"), _ = std::future::ready(()) => {} }
    drop(finish);
    reaped(&mut observer).await;
    absent(&registry, owner, id);
    registry.shutdown().unwrap();
}

#[tokio::test]
async fn dropped_cleanup_waiter_does_not_abort_native_join_or_reap() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let ready = launch(&registry, owner, "printf ready; read -r line").await;
    let id = ready.id;
    let owned = LocalSessionOwner::adopt(Handle::current(), ready);
    running(owned.handle()).await;
    let mut observer = owned.observe_cleanup();
    drop(owned.cancel());
    reaped(&mut observer).await;
    absent(&registry, owner, id);
    registry.shutdown().unwrap();
}

#[tokio::test]
async fn closed_actor_without_delivered_native_receipt_does_not_retire_capacity() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let ready = launch(&registry, owner, "printf ready; read -r line").await;
    running(&ready.handle).await;
    let RegisteredLocalSession {
        id,
        handle,
        completion: actual,
        registry: provenance,
    } = ready;
    let (deliver, gated) = oneshot::channel();
    let owned = LocalSessionOwner::adopt(
        Handle::current(),
        RegisteredLocalSession {
            id,
            handle,
            completion: gated,
            registry: provenance,
        },
    );
    let observer = owned.observe_cleanup();
    let cleanup = owned.cancel();
    let receipt = timeout(Duration::from_secs(5), actual)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(*observer.borrow(), CleanupState::Closing);
    // Actor/native actually ended, but this owner has not received proof yet.
    assert_eq!(registry.test_registered_session_count().unwrap(), 1);
    deliver.send(Ok(receipt)).unwrap();
    timeout(Duration::from_secs(5), cleanup.join())
        .await
        .unwrap()
        .unwrap();
    absent(&registry, owner, id);
    registry.shutdown().unwrap();
}

#[tokio::test]
async fn missing_completion_fails_closed_without_claiming_reap() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let ready = launch(&registry, owner, "printf ready; read -r line").await;
    running(&ready.handle).await;
    let RegisteredLocalSession {
        id,
        handle,
        completion: actual,
        registry: provenance,
    } = ready;
    let (deliver, gated) = oneshot::channel();
    let owned = LocalSessionOwner::adopt(
        Handle::current(),
        RegisteredLocalSession {
            id,
            handle,
            completion: gated,
            registry: provenance,
        },
    );
    let observer = owned.observe_cleanup();
    let cleanup = owned.cancel();
    // Keep the real proof outside the owner; a channel close alone must never reap.
    let _actual = timeout(Duration::from_secs(5), actual)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    drop(deliver);
    assert!(matches!(cleanup.join().await, Err(LocalStartError::Worker)));
    assert_eq!(*observer.borrow(), CleanupState::Failed);
    assert_eq!(registry.test_registered_session_count().unwrap(), 1);
    registry.reap_closed(id).unwrap(); // Trusted fixture has independently joined the child.
    registry.shutdown().unwrap();
}

#[tokio::test]
async fn native_error_is_not_a_join_proof_for_finish_or_cancel() {
    for finish in [true, false] {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let ready = launch(&registry, owner, "printf ready; read -r line").await;
        running(&ready.handle).await;
        let RegisteredLocalSession {
            id,
            handle,
            completion: actual,
            registry: provenance,
        } = ready;
        let (deliver, gated) = oneshot::channel();
        let owned = LocalSessionOwner::adopt(
            Handle::current(),
            RegisteredLocalSession {
                id,
                handle,
                completion: gated,
                registry: provenance,
            },
        );
        let observer = owned.observe_cleanup();
        owned.handle().close();
        let _real = timeout(Duration::from_secs(5), actual)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        deliver
            .send(Err(crate::LocalSessionError::unconfirmed(
                "fixture native join failed".into(),
            )))
            .unwrap();
        let result = if finish {
            owned.finish().await
        } else {
            owned.cancel().join().await
        };
        assert!(matches!(result, Err(LocalStartError::Native(_))));
        assert_eq!(*observer.borrow(), CleanupState::Failed);
        assert_eq!(registry.test_registered_session_count().unwrap(), 1);
        registry.reap_closed(id).unwrap(); // Only this fixture has the actual joined proof.
        registry.shutdown().unwrap();
    }
}

#[tokio::test]
async fn coincident_session_ids_only_reap_their_committing_registry() {
    let a = SessionRegistry::new();
    let b = SessionRegistry::new();
    let a_window = a.open_window().unwrap();
    let b_window = b.open_window().unwrap();
    let a_ready = launch(&a, a_window, "printf first; exit 0").await;
    let b_ready = launch(&b, b_window, "printf second; exit 7").await;
    assert_eq!(a_ready.id, b_ready.id);
    let b_owned = LocalSessionOwner::adopt(Handle::current(), b_ready);
    assert!(b_owned.is_managed_by(&b));
    assert!(b_owned.is_managed_by(&b.clone()));
    assert!(!b_owned.is_managed_by(&a));
    assert_eq!(
        timeout(Duration::from_secs(5), b_owned.finish())
            .await
            .unwrap()
            .unwrap()
            .native
            .exit_code,
        7
    );
    assert_eq!(b.test_registered_session_count().unwrap(), 0);
    assert_eq!(a.test_registered_session_count().unwrap(), 1);
    let a_owned = LocalSessionOwner::adopt(Handle::current(), a_ready);
    assert!(a_owned.is_managed_by(&a));
    assert!(!a_owned.is_managed_by(&b));
    assert_eq!(
        timeout(Duration::from_secs(5), a_owned.finish())
            .await
            .unwrap()
            .unwrap()
            .native
            .exit_code,
        0
    );
    assert_eq!(a.test_registered_session_count().unwrap(), 0);
    a.shutdown().unwrap();
    b.shutdown().unwrap();
}
