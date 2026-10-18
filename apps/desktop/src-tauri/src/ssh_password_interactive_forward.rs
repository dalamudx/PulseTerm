//! Both factors complete before opening the frozen next-hop channel; no PTY or shell.
use super::*;
use pulse_app::vault_owner::{PasswordForwardSink, PasswordInteractiveForwardSink};
use pulse_core::ssh_target::SshTarget;

pub(crate) struct JointForwardSink {
    inner: JointSessionSink,
    channel: Option<russh::ChannelStream<russh::client::Msg>>,
}
impl JointForwardSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        broker: Arc<Broker>,
        store: StateStore,
        source: SshTargetSource,
    ) -> std::result::Result<Self, bool> {
        if source.is_route_leaf() {
            return Err(transport.close_and_join().await.is_ok());
        }
        Ok(Self {
            inner: JointSessionSink::new_route(transport, selected, size, 0, broker, store, source)
                .await?,
            channel: None,
        })
    }
}
impl PasswordInteractiveForwardSink for JointForwardSink {}
impl PasswordTargetSink for JointForwardSink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        self.inner.authenticate(username, password)
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        drop(self.channel.take());
        self.inner.close_and_drain()
    }
}
impl PasswordForwardSink for JointForwardSink {
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
            self.inner.complete_second_factor(guard.clone()).await?;
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            let transport = self
                .inner
                .inner
                .base
                .transport
                .as_mut()
                .ok_or(VaultOwnerError::Denied)?;
            let channel = transport
                .client(&self.inner.inner.base.peer)
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
        if self.channel.is_none() || self.inner.inner.base.transport.is_none() {
            return Err(VaultOwnerError::Denied);
        }
        let mut guard = self
            .inner
            .inner
            .base
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if !guard.as_ref().is_some_and(|check| check()) {
            return Err(VaultOwnerError::Denied);
        }
        drop(guard.take());
        drop(guard);
        drop(self.inner.inner.base.completion.take());
        Ok((
            self.inner
                .inner
                .base
                .transport
                .take()
                .ok_or(VaultOwnerError::Denied)?,
            self.channel.take().ok_or(VaultOwnerError::Denied)?,
        ))
    }
}
