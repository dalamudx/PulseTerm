//! Explicit Agent + MFA for one frozen intermediate hop; ordinary Agent forwarding has no joint marker.
use super::*;
use pulse_app::vault_owner::AgentInteractiveForwardSink;

pub(crate) struct JointForwardSink {
    inner: ForwardSink,
}
impl JointForwardSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        broker: Arc<crate::ssh_session_adapter::interactive::Broker>,
        store: pulse_store::StateStore,
        source: SshKeyTargetSource,
    ) -> std::result::Result<Self, bool> {
        let mut inner = ForwardSink::new(transport, selected, size).await?;
        inner.inner.joint = Some(super::super::JointContext {
            broker,
            store,
            source,
        });
        Ok(Self { inner })
    }
    pub(crate) async fn close_and_drain(&mut self) -> Result<()> {
        self.inner.close_and_drain().await
    }
}
impl AgentInteractiveForwardSink for JointForwardSink {}
impl AgentSessionSink for JointForwardSink {
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
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.inner.prepare_authenticated_terminal(guard)
    }
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        self.inner.take_authenticated_root_parts()
    }
}
impl AgentForwardSink for JointForwardSink {
    type Transport = <ForwardSink as AgentForwardSink>::Transport;
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
