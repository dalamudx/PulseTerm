//! Single-owner binary delivery for a current-page attachment. Native adapters
//! pass a synchronous Raw Channel sender, never a Serde or unbounded staging queue.
use super::{AttachedTerminalView, TerminalViewBinding};
use crate::{
    attachment::{AttachmentError, AttachmentReceipt},
    registry::{RegisteredAttachment, RegistryError},
};
use std::sync::Arc;

/// Delivery failure revokes the renderer lease, never the application session.
#[derive(Debug, thiserror::Error)]
pub enum TerminalDeliveryError {
    #[error(transparent)]
    /// Source page or generation is no longer authorized.
    Current(#[from] RegistryError),
    #[error("terminal frame does not match this v2 binding")]
    /// Wrong version, malformed payload or mismatched generation.
    Wire,
    #[error("terminal binary delivery failed")]
    /// Native sender did not admit the bytes; no retry or inferred ACK.
    Send,
    #[error(transparent)]
    /// Ordered stream ended with a supervised protocol/credit failure.
    Attachment(#[from] AttachmentError),
    #[error("terminal completion supervisor lost")]
    /// No reliable completion receipt is available.
    Supervisor,
}

/// Exactly one owner forwards this stream. Cloned controls cannot keep a dropped
/// delivery alive. Delivery is NOT a draw receipt and never supplies ACK credit.
#[must_use]
pub struct TerminalViewDelivery {
    binding: Arc<TerminalViewBinding>,
    transport: Option<RegisteredAttachment>,
    detach_on_drop: bool,
    retain_ended: bool,
}
impl TerminalViewDelivery {
    /// Consume the opaque capture without permitting a caller to replace its source.
    pub fn new(view: AttachedTerminalView) -> Result<Self, TerminalDeliveryError> {
        view.binding.check_current()?;
        if view.transport.generation != view.binding.generation() {
            return Err(TerminalDeliveryError::Wire);
        }
        Ok(Self {
            binding: Arc::new(view.binding),
            transport: Some(view.transport),
            detach_on_drop: true,
            retain_ended: view.retain_ended,
        })
    }
    /// Native adapters must authenticate window, origin and nonce before using
    /// this control. Each operation still checks its source-fixed page/generation.
    pub fn binding(&self) -> Arc<TerminalViewBinding> {
        self.binding.clone()
    }
    /// Forward unchanged v2 bytes; no new queue, copies, inferred coordinates or
    /// retry after an uncertain send. An admitted send is NOT a renderer ACK.
    /// A send may race navigation; the frontend MUST discard its retired scope.
    /// Dropping this future revokes the lease even if another control survives.
    pub async fn forward(
        mut self,
        mut send: impl FnMut(Vec<u8>) -> bool,
    ) -> Result<AttachmentReceipt, TerminalDeliveryError> {
        let mut transport = self
            .transport
            .take()
            .ok_or(TerminalDeliveryError::Supervisor)?;
        while let Some(wire) = transport.wire.recv().await {
            self.binding.check_current()?;
            let frame = pulse_ipc::decode(&wire).map_err(|_| TerminalDeliveryError::Wire)?;
            let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload)
                .map_err(|_| TerminalDeliveryError::Wire)?;
            if fragment.generation != self.binding.generation() {
                return Err(TerminalDeliveryError::Wire);
            }
            if !send(wire) {
                return Err(TerminalDeliveryError::Send);
            }
        }
        let receipt = transport
            .completion
            .await
            .map_err(|_| TerminalDeliveryError::Supervisor)?
            .map_err(TerminalDeliveryError::Attachment)?;
        if self.retain_ended && receipt.boundary.lifecycle == pulse_term::session::Lifecycle::Ended
        {
            self.binding.check_current()?;
            self.detach_on_drop = false;
        }
        Ok(receipt)
    }
}
impl Drop for TerminalViewDelivery {
    fn drop(&mut self) {
        if self.detach_on_drop {
            let binding = &self.binding;
            let _ = binding
                .registry
                .detach_page(&binding.page, binding.id, binding.generation);
        }
    }
}
