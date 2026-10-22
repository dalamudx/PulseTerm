//! Native window capacity requires both a Destroyed event and successful physical I/O drain.
use super::*;
use pulse_app::window_directory::{WindowDirectory, WindowDrain, WindowLabel, WindowRegistration};
use tokio::sync::watch;
#[cfg(all(test, unix))]
#[path = "product_workspace_native_window_tests.rs"]
mod tests;
struct NativeState {
    revoked: Option<Result<()>>,
    drain: Option<WindowDrain>,
    destroyed: bool,
    io: Option<Result<()>>,
    completed: bool,
}
pub(crate) struct NativeBinding {
    directory: WindowDirectory,
    registration: Arc<WindowRegistration>,
    state: Mutex<NativeState>,
    done: watch::Sender<Option<Result<()>>>,
}
impl NativeBinding {
    pub(crate) fn new(
        directory: WindowDirectory,
        registration: Arc<WindowRegistration>,
    ) -> Result<Self> {
        directory
            .registration_owner(&registration)
            .map_err(|_| "workspace.denied")?;
        Ok(Self {
            directory,
            registration,
            state: Mutex::new(NativeState {
                revoked: None,
                drain: None,
                destroyed: false,
                io: None,
                completed: false,
            }),
            done: watch::channel(None).0,
        })
    }
    pub(crate) fn documents(&self) -> Arc<ProductDocuments> {
        self.directory.documents()
    }
    pub(crate) fn owner(&self) -> WindowToken {
        self.registration.owner()
    }
    pub(crate) fn label(&self) -> &WindowLabel {
        self.registration.label()
    }
    pub(crate) fn publish(&self) -> Result<()> {
        self.directory
            .publish(&self.registration)
            .map_err(|_| "workspace.denied")
    }
    pub(crate) fn revoke(&self) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "workspace.shutdown_failed")?;
        if let Some(result) = state.revoked {
            return result;
        }
        let result = match self.directory.begin_close(self.label()) {
            Ok(drain) => {
                state.drain = Some(drain);
                Ok(())
            }
            Err(_) => Err("workspace.shutdown_failed"),
        };
        state.revoked = Some(result);
        result
    }
    // Called only by the callback for this exact native window incarnation.
    // A builder error is not proof of native Destroyed. Keep directory credit and fail exit.
    #[cfg(any(
        test,
        all(feature = "workspace-cli", any(target_os = "linux", windows))
    ))]
    pub(crate) fn creation_failed(&self) -> Result<()> {
        let _ = self.revoke();
        let mut state = self.state.lock().map_err(|_| "workspace.shutdown_failed")?;
        if state.completed {
            return Err("workspace.shutdown_failed");
        }
        state.completed = true;
        self.done
            .send_replace(Some(Err("workspace.shutdown_failed")));
        Ok(())
    }
    pub(crate) fn destroyed(&self) -> Result<()> {
        let revoked = self.revoke();
        let mut state = self.state.lock().map_err(|_| "workspace.shutdown_failed")?;
        state.destroyed = true;
        revoked.and(self.finish(&mut state))
    }
    pub(crate) fn io_finished(&self, result: Result<()>) -> Result<()> {
        let revoked = self.revoke();
        let mut state = self.state.lock().map_err(|_| "workspace.shutdown_failed")?;
        if state.io.is_some() {
            return Err("workspace.shutdown_failed");
        }
        state.io = Some(result);
        revoked.and(self.finish(&mut state))
    }
    fn finish(&self, state: &mut NativeState) -> Result<()> {
        if state.completed {
            return self
                .done
                .borrow()
                .unwrap_or(Err("workspace.shutdown_failed"));
        }
        if !state.destroyed || state.io.is_none() {
            return Ok(());
        }
        let result = state
            .revoked
            .unwrap_or(Err("workspace.shutdown_failed"))
            .and(state.io.unwrap_or(Err("workspace.shutdown_failed")));
        let result = result.and_then(|()| {
            let drain = state.drain.take().ok_or("workspace.shutdown_failed")?;
            self.directory
                .complete_close(drain)
                .map_err(|_| "workspace.shutdown_failed")
        });
        state.completed = true;
        self.done.send_replace(Some(result));
        result
    }
    pub(crate) fn needs_close(&self) -> bool {
        self.state.lock().map_or(true, |state| !state.destroyed)
    }
    pub(crate) async fn wait(&self) -> Result<()> {
        let mut receipt = self.done.subscribe();
        loop {
            let result = *receipt.borrow();
            if let Some(result) = result {
                return result;
            }
            receipt
                .changed()
                .await
                .map_err(|_| "workspace.shutdown_failed")?;
        }
    }
}
