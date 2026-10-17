//! Current-page, owner-filtered bounded history reads. No ACK or control effects.
use super::*;
impl SessionRegistry {
    /// Search only this exact page's current binding, revalidated after the actor read.
    pub async fn history_search(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        request: pulse_term::HistorySearchRequest,
    ) -> Result<pulse_term::HistorySearch, RegistryError> {
        let (entry, generation) = {
            let state = self.lock()?;
            self.check_page_locked(&state, page)?;
            let entry = Self::entry(&state, page.owner, id)?;
            let binding = state
                .bindings
                .get(&id)
                .filter(|binding| binding.page == Some(page.page))
                .ok_or(RegistryError::Stale)?;
            (entry, binding.generation)
        };
        let _read = entry.capture.try_lock().map_err(|_| RegistryError::Busy)?;
        let result = entry.handle.history_search(request).await;
        self.check_page_binding(page, id, generation)?;
        Ok(result?)
    }
    /// Read retained primary history from this page's current binding.
    /// Revalidates the exact binding after the actor responds; never retries a stale cursor.
    pub async fn history_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        before: Option<usize>,
        revision: Option<u64>,
    ) -> Result<pulse_term::HistoryPage, RegistryError> {
        let (entry, generation) = {
            let state = self.lock()?;
            self.check_page_locked(&state, page)?;
            let entry = Self::entry(&state, page.owner, id)?;
            let binding = state
                .bindings
                .get(&id)
                .filter(|binding| binding.page == Some(page.page))
                .ok_or(RegistryError::Stale)?;
            (entry, binding.generation)
        };
        let _read = entry.capture.try_lock().map_err(|_| RegistryError::Busy)?;
        let result = entry.handle.history_page(before, revision).await;
        self.check_page_binding(page, id, generation)?;
        Ok(result?)
    }
}
