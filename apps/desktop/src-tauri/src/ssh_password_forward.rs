//! Intermediate route authentication. No PTY, shell, exec, or arbitrary target.
use super::*;
use pulse_app::vault_owner::{PasswordForwardSink, PasswordTargetSink};
use pulse_core::ssh_target::SshTarget;
use pulse_store::ssh_target_source::SshTargetSource;
pub(crate) struct ForwardSink {
    inner: Sink,
    channel: Option<russh::ChannelStream<russh::client::Msg>>,
}
impl ForwardSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
    ) -> std::result::Result<Self, bool> {
        Ok(Self {
            inner: Sink::new_route(transport, selected).await?,
            channel: None,
        })
    }
}
impl PasswordTargetSink for ForwardSink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(
        &mut self,
        guard: Guard,
    ) -> std::result::Result<(), VaultOwnerError> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        self.inner.authenticate(user, password)
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        drop(self.channel.take());
        self.inner.close_and_drain()
    }
}
impl PasswordForwardSink for ForwardSink {
    type Transport = (PinnedTransport, russh::ChannelStream<russh::client::Msg>);
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a SshTarget,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + 'a>> {
        Box::pin(async move {
            if !guard() || self.channel.is_some() {
                return Err(VaultOwnerError::Denied);
            }
            let transport = self
                .inner
                .transport
                .as_mut()
                .ok_or(VaultOwnerError::Denied)?;
            let channel = transport
                .client(&self.inner.peer)
                .map_err(|_| VaultOwnerError::Denied)?
                .channel_open_direct_tcpip(target.host(), u32::from(target.port()), "127.0.0.1", 0)
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            self.channel = Some(channel.into_stream());
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            Ok(())
        })
    }
    fn take_authenticated_forward(
        &mut self,
    ) -> std::result::Result<Self::Transport, VaultOwnerError> {
        if self.channel.is_none() || self.inner.transport.is_none() {
            return Err(VaultOwnerError::Denied);
        }
        let mut guard = self
            .inner
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if !guard.as_ref().is_some_and(|check| check()) {
            return Err(VaultOwnerError::Denied);
        }
        // Authentication/channel replies completed. Lifetime admission stays on the physical stream;
        // releasing the finished credential guard allows the next independent hop action.
        drop(guard.take());
        drop(guard);
        drop(self.inner.completion.take());
        Ok((
            self.inner.transport.take().ok_or(VaultOwnerError::Denied)?,
            self.channel.take().ok_or(VaultOwnerError::Denied)?,
        ))
    }
}
