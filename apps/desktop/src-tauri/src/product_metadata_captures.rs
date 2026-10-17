//! Typed metadata edit receipts: current page, one slot, one use and bounded expiry.
use super::*;
use pulse_app::registry::PageLaunchScope;
use std::time::{Duration, Instant};
struct Capture<T> {
    token: Uuid,
    page: Arc<PageLaunchScope>,
    snapshot: T,
    expires: Instant,
}
pub(super) struct MetadataCaptures<T> {
    capture: Mutex<Option<Capture<T>>>,
    failure: &'static str,
}
impl<T> MetadataCaptures<T> {
    pub(super) fn new(failure: &'static str) -> Self {
        Self {
            capture: Mutex::new(None),
            failure,
        }
    }
    pub(super) fn retire(&self) {
        if let Ok(mut slot) = self.capture.lock() {
            slot.take();
        }
    }
    pub(super) fn publish(
        &self,
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        snapshot: T,
    ) -> Result<Uuid> {
        let mut slot = self.capture.lock().map_err(|_| self.failure)?;
        registry
            .check_launch_page(&page)
            .map_err(|_| "workspace.denied")?;
        let token = Uuid::new_v4();
        *slot = Some(Capture {
            token,
            page,
            snapshot,
            expires: Instant::now() + Duration::from_secs(300),
        });
        Ok(token)
    }
    pub(super) fn take(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<T> {
        let mut slot = self.capture.lock().map_err(|_| self.failure)?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let current = slot.as_ref().ok_or("workspace.denied")?;
        if current.token != token
            || !Arc::ptr_eq(&current.page, page)
            || Instant::now() >= current.expires
        {
            return Err("workspace.denied");
        }
        Ok(slot.take().ok_or("workspace.denied")?.snapshot)
    }
    #[cfg(all(unix, feature = "workspace-ssh"))]
    pub(super) fn borrow(
        &self,
        registry: &SessionRegistry,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
    ) -> Result<T>
    where
        T: Clone,
    {
        let slot = self.capture.lock().map_err(|_| self.failure)?;
        registry
            .check_launch_page(page)
            .map_err(|_| "workspace.denied")?;
        let current = slot.as_ref().ok_or("workspace.denied")?;
        if current.token != token
            || !Arc::ptr_eq(&current.page, page)
            || Instant::now() >= current.expires
        {
            return Err("workspace.denied");
        }
        Ok(current.snapshot.clone())
    }
    #[cfg(test)]
    pub(super) fn expire_for_test(&self) -> Result<()> {
        self.capture
            .lock()
            .map_err(|_| "fixture.poison")?
            .as_mut()
            .ok_or("fixture.missing")?
            .expires = Instant::now();
        Ok(())
    }
}
