//! Current-document binary v2 view binding. The adapter MUST authenticate the
//! native window, local origin and document nonce before obtaining its page scope.
//! No session creation, arbitrary executable, file path, CSS, or physical-write receipt.
use crate::{
    attachment::AttachmentConfig,
    registry::{
        CapturedQueryPalette, PageLaunchScope, RegisteredAttachment, RegistryError, SessionId,
        SessionRegistry,
    },
};
use std::sync::Arc;
#[path = "terminal_view_delivery.rs"]
mod delivery;
pub use delivery::{TerminalDeliveryError, TerminalViewDelivery};

/// Source-bound operations cannot be retargeted by replacing public IDs or generations.
/// Drop detaches only this lease; application-owned sessions remain alive.
#[must_use]
pub struct TerminalViewBinding {
    registry: SessionRegistry,
    page: Arc<PageLaunchScope>,
    id: SessionId,
    generation: u64,
    detach_on_drop: bool,
}
/// Opaque, source-fixed capture. Drawing metadata is NOT authority. Use delivery
/// for ordered frames and acknowledge ONLY after the renderer's callback/paint gate.
#[must_use]
pub struct AttachedTerminalView {
    /// Source-fixed current-document operations, not a session owner.
    binding: TerminalViewBinding,
    /// Bounded ordered binary stream and completion observation.
    transport: RegisteredAttachment,
    /// Base palette at this capture; validate authority again before later use.
    palette: CapturedQueryPalette,
}
impl AttachedTerminalView {
    /// Read drawing data captured at this boundary after rechecking authority.
    /// Returned data never substitutes for the source-fixed control.
    pub fn captured_palette(&self) -> Result<&CapturedQueryPalette, RegistryError> {
        self.binding.check_current()?;
        Ok(&self.palette)
    }
}
impl TerminalViewBinding {
    /// Capture and subscribe at the actor boundary, then read that capture
    /// palette under current-page authorization. Cancellation cannot install a late view.
    pub async fn attach_v2(
        registry: &SessionRegistry,
        page: Arc<PageLaunchScope>,
        id: SessionId,
        config: AttachmentConfig,
    ) -> Result<AttachedTerminalView, RegistryError> {
        let transport = registry.attach_page_v2(&page, id, config).await?;
        let binding = Self {
            registry: registry.clone(),
            page,
            id,
            generation: transport.generation,
            detach_on_drop: true,
        };
        let palette = binding.captured_palette()?;
        Ok(AttachedTerminalView {
            binding,
            transport,
            palette,
        })
    }
    /// Registry-assigned generation. This integer by itself grants no permission.
    pub fn generation(&self) -> u64 {
        self.generation
    }
    /// Instantaneous source/page/generation check; not permission for later work.
    pub fn check_current(&self) -> Result<(), RegistryError> {
        self.registry
            .check_page_binding(&self.page, self.id, self.generation)
    }
    /// Revalidate and read the base palette of this exact capture, not live colors.
    pub fn captured_palette(&self) -> Result<CapturedQueryPalette, RegistryError> {
        self.registry
            .captured_query_palette_page(&self.page, self.id, self.generation)
    }
    /// Confirm exactly the captured screen, under the original document and view
    /// generation. Equal does not confirm live output or clear divergence hints.
    pub fn confirm_capture(
        &self,
        rendered: crate::registry::RenderedScreen,
    ) -> Result<crate::registry::VisibleComparison, RegistryError> {
        self.registry
            .compare_bootstrap_page(&self.page, self.id, self.generation, rendered)
    }
    /// Wait for the existing per-session confirmation allowance without changing
    /// its 500ms rule or upgrading this binding's original page/generation.
    pub async fn confirm_capture_when_ready(
        &self,
        rendered: crate::registry::RenderedScreen,
    ) -> Result<crate::registry::VisibleComparison, RegistryError> {
        self.registry
            .compare_bootstrap_page_when_ready(&self.page, self.id, self.generation, rendered)
            .await
    }
    /// Observational waiting hook only; confirmation still uses original page/gen.
    pub async fn confirm_capture_observed(
        &self,
        rendered: crate::registry::RenderedScreen,
        waiting: impl FnOnce() + Send,
    ) -> Result<crate::registry::VisibleComparison, RegistryError> {
        self.registry
            .compare_bootstrap_page_when_ready_observed(
                &self.page,
                self.id,
                self.generation,
                rendered,
                waiting,
            )
            .await
    }
    /// Forward ordered frame credit after drawing; the renderer supplies evidence.
    pub fn acknowledge(&self, sequence: u32) -> Result<(), RegistryError> {
        self.registry
            .acknowledge_page(&self.page, self.id, self.generation, sequence)
    }
    /// Wait for the delivery supervisor's Commit ACK validation, never inferred
    /// from a native acknowledge() queue-admission response. Caller sets deadline.
    pub async fn wait_bootstrap_ready(&self) -> Result<(), RegistryError> {
        self.registry
            .wait_page_bootstrap(&self.page, self.id, self.generation)
            .await
    }
    /// Admission only. Already admitted bytes cannot be retracted; do not retry on
    /// a late error unless physical-write status is independently known.
    pub async fn input(&self, bytes: Vec<u8>) -> Result<(), RegistryError> {
        self.registry
            .input_page_bound(&self.page, self.id, self.generation, bytes)
            .await
    }
    /// Compatibility API; reattach from authoritative backend grid after success.
    pub async fn resize(&self, size: pulse_core::TermSize) -> Result<(), RegistryError> {
        self.resize_with_receipt(size).await.map(|_| ())
    }
    /// Transport-specific layout receipt, never remote kernel proof or input permission.
    pub async fn resize_with_receipt(
        &self,
        size: pulse_core::TermSize,
    ) -> Result<pulse_term::session::ResizeReceipt, RegistryError> {
        self.registry
            .resize_page_bound_with_receipt(&self.page, self.id, self.generation, size)
            .await
    }
    /// Revoke just this lease, preserving application-owned session lifetime.
    pub fn detach(mut self) -> Result<(), RegistryError> {
        let result = self
            .registry
            .detach_page(&self.page, self.id, self.generation);
        self.detach_on_drop = false;
        result
    }
}
impl Drop for TerminalViewBinding {
    fn drop(&mut self) {
        if self.detach_on_drop {
            let _ = self
                .registry
                .detach_page(&self.page, self.id, self.generation);
        }
    }
}
#[cfg(all(test, unix))]
#[path = "terminal_view_pty_tests.rs"]
mod pty_tests;
#[cfg(test)]
#[path = "terminal_view_tests.rs"]
mod tests;
