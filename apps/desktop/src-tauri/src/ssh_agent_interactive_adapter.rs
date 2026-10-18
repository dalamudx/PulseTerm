//! Explicit saved-Agent plus MFA adapter. Ordinary Agent use has no joint marker.
use super::{Guard, JointContext, Result, SessionSink};
use pulse_app::vault_owner::{
    AgentInteractiveSessionSink, AgentSessionSink, AuthenticatedProtocolParts,
};
use pulse_core::{TermSize, ssh_agent_identity::SshAgentIdentity};
use pulse_ssh::peer::PinnedTransport;
use pulse_store::{StateStore, ssh_key_target_source::SshKeyTargetSource};
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
pub(crate) struct JointSessionSink {
    inner: SessionSink,
}
impl JointSessionSink {
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        history: usize,
        broker: Arc<crate::ssh_session_adapter::interactive::Broker>,
        store: StateStore,
        source: SshKeyTargetSource,
    ) -> std::result::Result<Self, bool> {
        let mut inner = SessionSink::new(transport, selected, size, history).await?;
        inner.joint = Some(JointContext {
            broker,
            store,
            source,
        });
        Ok(Self { inner })
    }
    pub(crate) fn set_palette(&mut self, config: pulse_app::TerminalStartConfig) {
        self.inner.set_palette(config);
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.inner.attach_parents(parents);
    }
    pub(crate) fn observe_native_admission(&self) -> Guard {
        self.inner.observe_native_admission()
    }
    pub(crate) async fn close_and_drain(&mut self) -> Result<()> {
        self.inner.close_and_drain().await
    }
}
impl AgentInteractiveSessionSink for JointSessionSink {}
impl AgentSessionSink for JointSessionSink {
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
