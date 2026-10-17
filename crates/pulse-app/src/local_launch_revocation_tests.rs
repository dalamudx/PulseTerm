//! Deterministic current-thread check: the worker cannot run until the caller yields.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use super::*;
use tokio::time::{Duration, timeout};

#[tokio::test(flavor = "current_thread")]
async fn retired_window_before_worker_poll_never_reaches_native_setup() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let size = TermSize::new(32, 8).unwrap();
    // With a current owner, the deliberately missing executable must reach native
    // setup and fail there. No subprocess or user-owned file is needed as a marker.
    let absent = || PtyCommand::new("/definitely/absent/pulseterm-child-test");
    let pending = start_local_registered(&registry, owner, absent(), size, 16).unwrap();
    let outcome = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap();
    assert!(matches!(outcome, Err(LocalStartError::Native(_))));

    let pending = start_local_registered(&registry, owner, absent(), size, 16).unwrap();
    // The worker is scheduled on this same executor and has not been polled yet.
    registry.close_window(owner).unwrap();
    let outcome = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
}
