//! Application-owned settings admission. Page captures stay in each SettingsOwner.
use super::*;
struct IoState {
    closed: bool,
    directory: Option<PathBuf>,
}
pub(crate) struct SettingsIo {
    state: Mutex<IoState>,
    credit: Arc<Semaphore>,
    idle: watch::Sender<bool>,
}
pub(super) struct IoCredit {
    owner: Arc<SettingsIo>,
    permit: Option<OwnedSemaphorePermit>,
}
impl Drop for IoCredit {
    fn drop(&mut self) {
        // Admission and release share this lock, so an old completion cannot overwrite
        // the new holder's busy receipt after releasing its semaphore permit.
        let _state = self
            .owner
            .state
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        drop(self.permit.take());
        self.owner.idle.send_replace(true);
    }
}
impl SettingsIo {
    pub(crate) fn new() -> Self {
        Self {
            state: Mutex::new(IoState {
                closed: false,
                directory: None,
            }),
            credit: Arc::new(Semaphore::new(1)),
            idle: watch::channel(true).0,
        }
    }
    pub(super) fn reserve(self: &Arc<Self>) -> Result<IoCredit> {
        let state = self.state.lock().map_err(|_| "workspace.settings_failed")?;
        if state.closed {
            return Err("workspace.denied");
        }
        let permit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        self.idle.send_replace(false);
        Ok(IoCredit {
            owner: self.clone(),
            permit: Some(permit),
        })
    }
    pub(super) fn location(&self, directory: &PathBuf) -> Result<()> {
        let mut state = self.state.lock().map_err(|_| "workspace.settings_failed")?;
        if state.closed || state.directory.as_ref().is_some_and(|old| old != directory) {
            return Err("workspace.denied");
        }
        state.directory = Some(directory.clone());
        Ok(())
    }
    pub(crate) async fn shutdown(&self) -> Result<()> {
        {
            let mut state = self.state.lock().map_err(|_| "workspace.settings_failed")?;
            state.closed = true;
            self.credit.close();
        }
        let mut idle = self.idle.subscribe();
        while !*idle.borrow_and_update() {
            idle.changed()
                .await
                .map_err(|_| "workspace.settings_failed")?;
        }
        Ok(())
    }
}
#[cfg(all(test, unix))]
#[path = "product_settings_io_tests.rs"]
mod tests;
