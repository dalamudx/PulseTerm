//! Local Agent authentication for an intermediate, exact-target SSH hop. No remote Agent forwarding.
use super::*;
use pulse_app::vault_owner::AgentForwardSink;
use pulse_core::ssh_target::SshTarget;
pub(crate) struct ForwardSink {
    inner: SessionSink,
    channel: Option<russh::ChannelStream<russh::client::Msg>>,
}
impl ForwardSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
    ) -> std::result::Result<Self, bool> {
        Ok(Self {
            inner: SessionSink::new(transport, selected, size, 0).await?,
            channel: None,
        })
    }
}
impl AgentSessionSink for ForwardSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate_agent<'a>(
        &'a mut self,
        identity: &'a SshAgentIdentity,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        self.inner.authenticate_agent(identity, guard)
    }
    fn prepare_authenticated_terminal(
        &mut self,
        _: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        Err(VaultOwnerError::Denied)
    }
}
impl AgentForwardSink for ForwardSink {
    type Transport = (PinnedTransport, russh::ChannelStream<russh::client::Msg>);
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a SshTarget,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async move {
            if !guard() || self.channel.is_some() {
                return Err(VaultOwnerError::Denied);
            }
            let raw = &mut self.inner.terminal.base;
            let transport = raw.transport.as_mut().ok_or(VaultOwnerError::Denied)?;
            let channel = transport
                .client(&raw.peer)
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
    fn take_authenticated_forward(&mut self) -> Result<Self::Transport> {
        let raw = &mut self.inner.terminal.base;
        if self.channel.is_none() || raw.transport.is_none() {
            return Err(VaultOwnerError::Denied);
        }
        let mut guard = raw
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if !guard.as_ref().is_some_and(|check| check()) {
            return Err(VaultOwnerError::Denied);
        }
        drop(guard.take());
        drop(guard);
        drop(raw.completion.take());
        Ok((
            raw.transport.take().ok_or(VaultOwnerError::Denied)?,
            self.channel.take().ok_or(VaultOwnerError::Denied)?,
        ))
    }
}
impl ForwardSink {
    pub(crate) async fn close_and_drain(&mut self) -> Result<()> {
        drop(self.channel.take());
        self.inner.close_and_drain().await
    }
}
