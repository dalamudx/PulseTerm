//! One selected backend-discovered Agent key on the same verified terminal peer.
use super::{PasswordSessionSink, PasswordTargetSink, SessionSink as TerminalSink};
use pulse_app::vault_owner::{AgentSessionSink, AuthenticatedProtocolParts, VaultOwnerError};
use pulse_core::{TermSize, ssh_agent_identity::SshAgentIdentity};
use pulse_ssh::{agent::AgentConnection, peer::PinnedTransport};
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
type Result<T> = std::result::Result<T, VaultOwnerError>;
#[path = "ssh_agent_forward.rs"]
pub(crate) mod forward;
#[path = "ssh_agent_interactive_adapter.rs"]
pub(crate) mod interactive;
struct JointContext {
    broker: Arc<super::interactive::Broker>,
    store: pulse_store::StateStore,
    source: SshKeyTargetSource,
}
pub(crate) struct SessionSink {
    pub(super) terminal: TerminalSink,
    selected: (Uuid, Uuid),
    joint: Option<JointContext>,
}
impl SessionSink {
    pub(crate) fn set_palette(&mut self, palette: impl Into<pulse_app::TerminalStartConfig>) {
        self.terminal.set_palette(palette);
    }
    pub(crate) async fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        history: usize,
    ) -> std::result::Result<Self, bool> {
        Ok(Self {
            terminal: TerminalSink::new_route(transport, selected, size, history).await?,
            selected,
            joint: None,
        })
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.terminal.attach_parents(parents);
    }
    pub(crate) async fn close_and_drain(&mut self) -> Result<()> {
        PasswordTargetSink::close_and_drain(&mut self.terminal).await
    }
    pub(crate) fn observe_native_admission(&self) -> Guard {
        let weak = self.terminal.observe_admission();
        Arc::new(move || weak.upgrade().is_some())
    }
}
impl AgentSessionSink for SessionSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
            && source.supports_agent_selection(host, identity)
            && self
                .terminal
                .base
                .transport
                .as_ref()
                .is_some_and(|transport| {
                    super::secret_target::peer_guard(source.password_source(), host, transport)
                        .is_ok_and(|guard| guard())
                })
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        PasswordTargetSink::retain_native_authorization(&mut self.terminal, guard)
    }
    fn authenticate_agent<'a>(
        &'a mut self,
        identity: &'a SshAgentIdentity,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            let agent = AgentConnection::discover(guard.clone())
                .await
                .map_err(|_| VaultOwnerError::Denied)?;
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            if let Some(joint) = &self.joint {
                joint
                    .store
                    .validate_ssh_key_target_source(joint.source.clone(), guard.clone())
                    .await
                    .map_err(|_| VaultOwnerError::Denied)?;
            }
            let base = &mut self.terminal.base;
            if let Some(joint) = &self.joint {
                joint
                    .store
                    .validate_ssh_key_target_source(joint.source.clone(), guard.clone())
                    .await
                    .map_err(|_| VaultOwnerError::Denied)?;
            }
            let transport = base.transport.as_mut().ok_or(VaultOwnerError::Denied)?;
            let client = transport
                .client(&base.peer)
                .map_err(|_| VaultOwnerError::Denied)?;
            if let Some(joint) = &self.joint {
                if !agent
                    .authenticate_interactive_first_factor(client, identity, guard.clone())
                    .await
                    .map_err(|_| VaultOwnerError::Denied)?
                {
                    return Ok(false);
                }
                let original = guard.clone();
                let store = joint.store.clone();
                let source = joint.source.clone();
                super::interactive::authenticate_client(
                    transport,
                    &base.peer,
                    &joint.broker,
                    identity.username(),
                    guard,
                    move || {
                        let store = store.clone();
                        let source = source.clone();
                        let original = original.clone();
                        async move {
                            store
                                .validate_ssh_key_target_source(source, original)
                                .await
                                .map_err(|_| VaultOwnerError::Denied)
                        }
                    },
                )
                .await
            } else {
                agent
                    .authenticate(client, identity, guard)
                    .await
                    .map_err(|_| VaultOwnerError::Denied)
            }
        })
    }
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        PasswordSessionSink::prepare_authenticated_terminal(&mut self.terminal, guard)
    }
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        PasswordSessionSink::take_authenticated_root_parts(&mut self.terminal)
    }
}
