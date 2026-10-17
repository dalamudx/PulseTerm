//! One original-page import approval. A revoked SQL operation keeps its bank slot until native exit.
use super::*;
use profiles::{ConfigImportCapture, ImportControl};
struct Slot {
    token: Uuid,
    capture: Arc<ConfigImportCapture>,
    control: Arc<ImportControl>,
}
pub(super) struct ConfigImportBank {
    slot: Mutex<Option<Slot>>,
}
impl ConfigImportBank {
    pub(super) fn new() -> Self {
        Self {
            slot: Mutex::new(None),
        }
    }
    pub(super) fn retire(&self) {
        if let Ok(mut slot) = self.slot.lock() {
            if let Some(value) = slot.as_ref() {
                value.control.revoke();
                if value.control.running() {
                    return;
                }
            }
            slot.take();
        }
    }
    pub(super) fn publish(
        &self,
        registry: &SessionRegistry,
        capture: Arc<ConfigImportCapture>,
    ) -> Result<Uuid> {
        registry
            .check_launch_page(capture.page())
            .map_err(|_| "workspace.denied")?;
        let mut slot = self
            .slot
            .lock()
            .map_err(|_| "workspace.config_unavailable")?;
        if let Some(value) = slot.as_ref() {
            if value.control.running() {
                return Err("workspace.busy");
            }
            value.control.revoke();
        }
        let token = Uuid::new_v4();
        *slot = Some(Slot {
            token,
            capture,
            control: ImportControl::new(),
        });
        Ok(token)
    }
    pub(super) fn claim(
        &self,
        registry: &SessionRegistry,
        page: &Arc<pulse_app::registry::PageLaunchScope>,
        token: Uuid,
    ) -> Result<(Arc<ConfigImportCapture>, Arc<ImportControl>)> {
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let slot = self
            .slot
            .lock()
            .map_err(|_| "workspace.config_unavailable")?;
        let value = slot.as_ref().ok_or("workspace.denied")?;
        if value.token != token || !Arc::ptr_eq(value.capture.page(), page) {
            return Err("workspace.denied");
        }
        value.control.claim()?;
        Ok((value.capture.clone(), value.control.clone()))
    }
    pub(super) fn cancel(
        &self,
        registry: &SessionRegistry,
        page: &Arc<pulse_app::registry::PageLaunchScope>,
        token: Uuid,
    ) -> Result<()> {
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let slot = self
            .slot
            .lock()
            .map_err(|_| "workspace.config_unavailable")?;
        let value = slot.as_ref().ok_or("workspace.denied")?;
        if value.token != token || !Arc::ptr_eq(value.capture.page(), page) {
            return Err("workspace.denied");
        }
        value.control.revoke();
        Ok(())
    }
}
