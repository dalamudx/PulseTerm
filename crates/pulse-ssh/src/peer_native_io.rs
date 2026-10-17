//! Native stream lifetime, independent of keyless evidence revocation and receipt waiter lifetime.
use super::PeerError;
use std::sync::{Arc, Mutex};
use tokio::sync::watch;
/// Key-free observation of completed owned stream destruction and admission disposal.
/// Does not prove SDK task join, remote process exit, authentication or credential permission.
/// ```compile_fail
/// fn dump(v:pulse_ssh::peer::NativeIoCompletion){println!("{v:?}");}
/// ```
pub struct NativeIoCompletion {
    changed: watch::Receiver<bool>,
}
impl NativeIoCompletion {
    pub(super) fn duplicate(&self) -> Self {
        Self {
            changed: self.changed.clone(),
        }
    }
    /// True only after underlying stream destruction and retained reservation disposal complete.
    pub fn is_complete(&self) -> bool {
        *self.changed.borrow()
    }
    /// Dropping/cancelling this waiter cannot release stream ownership or its reservation.
    /// An abandoned producer without completion is a failure, not successful native exit.
    pub async fn wait(mut self) -> Result<(), PeerError> {
        loop {
            if self.is_complete() {
                return Ok(());
            }
            self.changed
                .changed()
                .await
                .map_err(|_| PeerError::Transport)?;
        }
    }
}
struct State {
    alive: bool,
    admission: Option<Arc<dyn Send + Sync>>,
}
pub(super) struct IoLifetime {
    state: Mutex<State>,
    done: watch::Sender<bool>,
}
impl IoLifetime {
    pub(super) fn new() -> Self {
        Self {
            state: Mutex::new(State {
                alive: true,
                admission: None,
            }),
            done: watch::channel(false).0,
        }
    }
    pub(super) fn receipt(&self) -> NativeIoCompletion {
        NativeIoCompletion {
            changed: self.done.subscribe(),
        }
    }
    pub(super) fn retain(
        &self,
        admission: Arc<dyn Send + Sync>,
    ) -> Result<NativeIoCompletion, PeerError> {
        let result = {
            let mut state = self.state.lock().map_err(|_| PeerError::Stale)?;
            if !state.alive {
                Err(PeerError::Stale)
            } else if state.admission.is_some() {
                Err(PeerError::AdmissionBusy)
            } else {
                state.admission = Some(admission.clone());
                Ok(())
            }
        };
        drop(admission);
        result?;
        Ok(self.receipt())
    }
    pub(super) fn begin_drop(&self) -> Option<Arc<dyn Send + Sync>> {
        let mut state = self.state.lock().unwrap_or_else(|p| p.into_inner());
        state.alive = false;
        state.admission.take()
    }
    pub(super) fn finished(&self) {
        self.done.send_replace(true);
    }
}
#[cfg(test)]
#[path = "peer_native_io_tests.rs"]
mod tests;
