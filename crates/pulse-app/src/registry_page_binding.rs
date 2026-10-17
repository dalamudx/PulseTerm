//! Current-document operations on explicitly page-bound views; not IPC authorization.
use super::*;
#[cfg(test)]
#[path = "registry_page_binding_tests.rs"]
mod tests;
impl SessionRegistry {
    pub(super) fn page_binding<'a>(
        &self,
        state: &'a State,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
    ) -> Result<&'a Binding, RegistryError> {
        self.check_page_locked(state, page)?;
        Self::entry(state, page.owner, id)?;
        state
            .bindings
            .get(&id)
            .filter(|binding| binding.generation == generation && binding.page == Some(page.page))
            .ok_or(RegistryError::Stale)
    }
    /// Irreversible current-document session-close admission, NOT a native reaping
    /// receipt. Keep registration for the same-source root owner's join/reap.
    pub fn request_close_page_session(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
    ) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        self.check_page_locked(&state, page)?;
        let entry = Self::entry(&state, page.owner, id)?;
        entry.handle.close();
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        Ok(())
    }
    /// Check the page AND view generation, including the page which captured the view.
    /// A legacy window-only binding cannot be promoted by supplying a current scope.
    pub fn check_page_binding(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
        self.page_binding(&*self.lock()?, page, id, generation)
            .map(|_| ())
    }
    /// ACK admission is atomic with current-page and binding checks. A rejected old
    /// page cannot forward ACK credit or terminate the current renderer generation.
    pub fn acknowledge_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        sequence: u32,
    ) -> Result<(), RegistryError> {
        let state = self.lock()?;
        self.page_binding(&state, page, id, generation)?
            .handle
            .acknowledge(generation, sequence)?;
        Ok(())
    }
    /// Only this current page and generation may detach; keep the session alive.
    pub fn detach_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
        let mut state = self.lock()?;
        self.page_binding(&state, page, id, generation)?;
        if let Some(binding) = state.bindings.remove(&id) {
            binding.handle.detach();
        }
        Ok(())
    }
    /// Same-lock page, generation and exact capture palette read. Metadata is not
    /// permission to render later; remote OSC overrides remain in the replay bytes.
    pub fn captured_query_palette_page(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
    ) -> Result<CapturedQueryPalette, RegistryError> {
        let state = self.lock()?;
        let binding = self.page_binding(&state, page, id, generation)?;
        let (boundary, palette) = binding
            .handle
            .bootstrap_palette()
            .ok_or(RegistryError::Stale)?;
        Ok(CapturedQueryPalette {
            generation,
            boundary: boundary.clone(),
            palette: palette.clone(),
        })
    }
    /// Wait for validated bootstrap ACKs on this exact page-bound lease. The
    /// caller owns its deadline; revoked/retired drivers cannot remain ready.
    pub async fn wait_page_bootstrap(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
    ) -> Result<(), RegistryError> {
        let handle = self
            .page_binding(&*self.lock()?, page, id, generation)?
            .handle
            .clone();
        handle.wait_bootstrap_ready().await?;
        self.check_page_binding(page, id, generation)
    }
    /// Compatibility API retaining original page/generation authorization.
    pub async fn resize_page_bound(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        size: TermSize,
    ) -> Result<(), RegistryError> {
        self.resize_page_bound_with_receipt(page, id, generation, size)
            .await
            .map(|_| ())
    }
    /// Original page/generation authorize the configured transport's layout operation.
    /// Later navigation cannot retract an admitted effect or publish its result to a new page.
    pub async fn resize_page_bound_with_receipt(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        size: TermSize,
    ) -> Result<pulse_term::session::ResizeReceipt, RegistryError> {
        let binding = self
            .page_binding(&*self.lock()?, page, id, generation)?
            .handle
            .clone();
        let receipt = binding.resize_with_receipt(size).await?;
        let state = self.lock()?;
        self.check_page_locked(&state, page)?;
        Self::entry(&state, page.owner, id)?;
        Ok(receipt)
    }
    /// User input admission, not a physical write receipt. Scope Drop/rotation detaches
    /// this page's binding, so actor-side cancellation also rejects a queued old input.
    /// Already admitted input is not retractable; a late error must not imply safe retry.
    pub async fn input_page_bound(
        &self,
        page: &PageLaunchScope,
        id: SessionId,
        generation: u64,
        bytes: Vec<u8>,
    ) -> Result<(), RegistryError> {
        let handle = self
            .page_binding(&*self.lock()?, page, id, generation)?
            .handle
            .clone();
        handle.input(bytes).await?;
        self.check_page_binding(page, id, generation)
    }
}
