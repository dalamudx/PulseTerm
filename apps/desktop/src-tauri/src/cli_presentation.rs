//! A presentation-ready marker is metadata, not action or session authority.
//! It is bound to the original confirmed page Arc, never just its nonce.
//! Callers bound the wait within their metadata deadline and recheck admission
//! before any effect. Canceling one waiter does not revoke shared readiness.
use pulse_app::{
    product_document::ProductDocuments,
    registry::{PageLaunchScope, WindowToken},
};
use std::sync::{Arc, Mutex};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;

/// Content-free failure; never includes document, request, or secret material.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("workspace.cli_presentation_unavailable")]
pub struct Error;

#[derive(Default)]
struct State {
    ready: Option<Arc<PageLaunchScope>>,
    closed: bool,
}
/// Root-owned, revocable presentation metadata, independent of native I/O credits.
/// A successful wait does not grant execution or replace current-page admission.
#[derive(Default)]
pub struct Presentation {
    state: Mutex<State>,
    changed: Notify,
}
impl Presentation {
    /// Called only after the adapter validates the actual source and current nonce.
    /// Same-page repetition is idempotent; stale/foreign pages cannot overwrite it.
    pub fn mark(
        &self,
        documents: &ProductDocuments,
        owner: WindowToken,
        original: Arc<PageLaunchScope>,
    ) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error)?;
        if state.closed {
            return Err(Error);
        }
        let current = documents.current_native(owner).map_err(|_| Error)?;
        if !Arc::ptr_eq(&current, &original) {
            return Err(Error);
        }
        state.ready = Some(original);
        self.changed.notify_waiters();
        Ok(())
    }
    /// Navigation clears only the marker, not any registered native session.
    /// Invoke after the document controller has synchronously revoked the old page.
    pub fn navigation(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error)?;
        state.ready = None;
        self.changed.notify_waiters();
        Ok(())
    }
    /// Permanently closes this root marker and wakes pending metadata waiters.
    pub fn close(&self) -> Result<(), Error> {
        let mut state = self.state.lock().map_err(|_| Error)?;
        state.closed = true;
        state.ready = None;
        self.changed.notify_waiters();
        Ok(())
    }
    /// Waits for presentation metadata only. Holds no native/SQL/CPU permit.
    pub async fn wait(
        &self,
        documents: &ProductDocuments,
        owner: WindowToken,
        original: &Arc<PageLaunchScope>,
        cancelled: &CancellationToken,
    ) -> Result<(), Error> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let ready = {
                let state = self.state.lock().map_err(|_| Error)?;
                if state.closed || cancelled.is_cancelled() {
                    return Err(Error);
                }
                let current = documents.current_native(owner).map_err(|_| Error)?;
                if !Arc::ptr_eq(&current, original) {
                    return Err(Error);
                }
                state
                    .ready
                    .as_ref()
                    .is_some_and(|ready| Arc::ptr_eq(ready, original))
            };
            if ready {
                return Ok(());
            }
            tokio::select! { biased;
                _ = cancelled.cancelled() => return Err(Error),
                _ = &mut changed => {},
            }
        }
    }
}
#[cfg(test)]
#[path = "cli_presentation_tests.rs"]
mod tests;
