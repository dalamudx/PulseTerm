//! Explicit two-factor intermediate adapter. No terminal interface or arbitrary destination.
use super::*;
use pulse_app::vault_owner::{PrivateKeyForwardSink, PrivateKeyInteractiveForwardSink};
use pulse_core::ssh_target::SshTarget;
pub(crate) struct JointForwardSink {
    inner: crate::ssh_private_key_adapter::forward::ForwardSink,
}
impl JointForwardSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        broker: Arc<Broker>,
    ) -> std::result::Result<Self, bool> {
        let mut inner =
            crate::ssh_private_key_adapter::forward::ForwardSink::new(transport, selected).await?;
        inner.require_interactive(broker);
        Ok(Self { inner })
    }
}
impl PrivateKeyInteractiveForwardSink for JointForwardSink {}
impl PrivateKeyTargetSink for JointForwardSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        key: &'a [u8],
        passphrase: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        self.inner
            .authenticate(username, key, passphrase, confirmation)
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.inner.close_and_drain()
    }
}
impl PrivateKeyForwardSink for JointForwardSink {
    type Transport = (PinnedTransport, russh::ChannelStream<russh::client::Msg>);
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a SshTarget,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        self.inner.prepare_authenticated_forward(target, guard)
    }
    fn take_authenticated_forward(&mut self) -> Result<Self::Transport> {
        self.inner.take_authenticated_forward()
    }
}
