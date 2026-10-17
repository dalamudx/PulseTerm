//! Physical lifetime of registered local and protocol native supervisors, not view permission.
//! Sealing prevents new native admission, including an earlier preference read.
//! Keep the runtime alive through drain; this is not descendant-tree/OS teardown proof.
use crate::{local_launch::LocalStartError, registry::RegistryError};
use tokio::sync::watch;

const MAX_WORKERS: usize = 64;
#[derive(Clone, Copy, Default)]
struct Status {
    sealed: bool,
    active: usize,
    failed: usize,
}
#[derive(Clone)]
pub(crate) struct RegisteredNativeWorkers {
    status: watch::Sender<Status>,
}
impl RegisteredNativeWorkers {
    pub(crate) fn new() -> Self {
        let (status, _) = watch::channel(Status::default());
        Self { status }
    }
    pub(crate) fn reserve(&self) -> Result<NativeWork, RegistryError> {
        let mut result = Err(RegistryError::Forbidden);
        self.status.send_if_modified(|state| {
            if state.sealed {
                return false;
            }
            if state.active + state.failed >= MAX_WORKERS {
                result = Err(RegistryError::Busy);
                return false;
            }
            state.active += 1;
            result = Ok(());
            true
        });
        result?;
        Ok(NativeWork {
            status: self.status.clone(),
            started: false,
            resolved: false,
        })
    }
    pub(crate) fn seal(&self) -> RegisteredNativeDrain {
        self.status.send_modify(|state| state.sealed = true);
        RegisteredNativeDrain {
            status: self.status.subscribe(),
        }
    }
}
/// Completion of registered local/protocol native workers ONLY. Does not stop actors or
/// reap root records, close settings I/O, kill descendants, or grant any authority.
/// Caller must first request close on all root owners and retain its runtime.
/// Dropping this observation does not cancel native work or unseal admission.
#[must_use]
pub struct RegisteredNativeDrain {
    status: watch::Receiver<Status>,
}
impl RegisteredNativeDrain {
    /// An error is UNKNOWN cleanup, never permission to destroy the runtime/exit.
    pub async fn join(mut self) -> Result<(), LocalStartError> {
        loop {
            let state = *self.status.borrow_and_update();
            if state.failed != 0 {
                return Err(LocalStartError::Worker);
            }
            if state.sealed && state.active == 0 {
                return Ok(());
            }
            self.status
                .changed()
                .await
                .map_err(|_| LocalStartError::Worker)?;
        }
    }
}
// The queued supervisor owns this guard. It is armed BEFORE blocking setup.
// Aborting runtime supervision after that is failed/unknown, not a free slot.
pub(crate) struct NativeWork {
    status: watch::Sender<Status>,
    started: bool,
    resolved: bool,
}
impl NativeWork {
    pub(crate) fn sealed(&self) -> bool {
        self.status.borrow().sealed
    }
    pub(crate) async fn wait_sealed(&self) {
        let mut state = self.status.subscribe();
        let _ = state.wait_for(|state| state.sealed).await;
    }
    pub(crate) fn start(&mut self) {
        self.started = true;
    }
    pub(crate) fn complete(mut self) {
        self.status.send_modify(|state| state.active -= 1);
        self.resolved = true;
    }
}
impl Drop for NativeWork {
    fn drop(&mut self) {
        if !self.resolved {
            self.status.send_modify(|state| {
                state.active -= 1;
                if self.started {
                    state.failed += 1;
                }
            });
        }
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    #[tokio::test]
    async fn sealing_waits_for_real_work_and_blocks_admission_without_polling() {
        let ledger = RegisteredNativeWorkers::new();
        let mut guard = ledger.reserve().unwrap();
        guard.start();
        let mut drain = Box::pin(ledger.seal().join());
        assert!(matches!(ledger.reserve(), Err(RegistryError::Forbidden)));
        assert!(matches!(
            drain.as_mut().poll(&mut Context::from_waker(Waker::noop())),
            Poll::Pending
        ));
        guard.complete();
        drain.await.unwrap();
    }
    #[tokio::test]
    async fn never_started_work_can_be_cancelled_but_unknown_started_work_cannot() {
        let ledger = RegisteredNativeWorkers::new();
        drop(ledger.reserve().unwrap());
        ledger.seal().join().await.unwrap();
        let ledger = RegisteredNativeWorkers::new();
        let mut guard = ledger.reserve().unwrap();
        guard.start();
        drop(guard);
        assert!(matches!(
            ledger.seal().join().await,
            Err(LocalStartError::Worker)
        ));
    }
    #[tokio::test]
    async fn dropped_shutdown_waiter_does_not_release_or_reopen_admission() {
        let ledger = RegisteredNativeWorkers::new();
        let mut guard = ledger.reserve().unwrap();
        guard.start();
        drop(ledger.seal());
        assert_eq!(ledger.status.borrow().active, 1);
        assert!(matches!(ledger.reserve(), Err(RegistryError::Forbidden)));
        guard.complete();
        ledger.seal().join().await.unwrap();
    }
    #[test]
    fn failed_work_keeps_capacity_even_when_its_supervisor_exited() {
        let ledger = RegisteredNativeWorkers::new();
        for _ in 0..MAX_WORKERS {
            let mut guard = ledger.reserve().unwrap();
            guard.start();
            drop(guard);
        }
        assert!(matches!(ledger.reserve(), Err(RegistryError::Busy)));
    }
}
