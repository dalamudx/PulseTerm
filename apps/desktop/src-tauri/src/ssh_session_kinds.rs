//! Typed saved-identity branches share supervision, never credential-use authority.
use pulse_app::{
    protocol_session_owner::{ProtocolSessionOwner, StartedProtocolSession},
    registry::PageLaunchScope,
    vault_owner::{
        AgentSessionAction, AgentSessionGesture, AgentSessionIntent, InteractiveSessionAction,
        InteractiveSessionGesture, InteractiveSessionIntent, PageVaultCancellation, PageVaultOwner,
        PasswordSessionAction, PasswordSessionGesture, PasswordSessionIntent,
        PrivateKeySessionAction, PrivateKeySessionGesture, PrivateKeySessionIntent,
        VaultOwnerError,
    },
};
use pulse_ssh::{
    HostPin, HostTarget,
    managed_store::validated_pins,
    peer::{NativeIoCompletion, PinnedTransport},
};
use pulse_store::{
    StateStore, ssh_key_target_source::SshKeyTargetSource, ssh_target_source::SshTargetSource,
};
use std::sync::Arc;
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) enum Gesture {
    Password(PasswordSessionGesture),
    KeyInteractive(
        pulse_app::vault_owner::PrivateKeyInteractiveSessionGesture,
        Arc<super::interactive::Broker>,
    ),
    PrivateKey(PrivateKeySessionGesture),
    Agent(AgentSessionGesture),
    AgentInteractive(
        pulse_app::vault_owner::AgentInteractiveSessionGesture,
        Arc<super::interactive::Broker>,
    ),
    Interactive(InteractiveSessionGesture, Arc<super::interactive::Broker>),
}
pub(crate) enum Action {
    Password(PasswordSessionAction),
    KeyInteractive(
        pulse_app::vault_owner::PrivateKeyInteractiveSessionAction,
        Arc<super::interactive::Broker>,
    ),
    PrivateKey(PrivateKeySessionAction),
    Agent(AgentSessionAction),
    AgentInteractive(
        pulse_app::vault_owner::AgentInteractiveSessionAction,
        Arc<super::interactive::Broker>,
    ),
    Interactive(InteractiveSessionAction, Arc<super::interactive::Broker>),
}
pub(crate) enum Source {
    Password(SshTargetSource),
    PrivateKey(SshKeyTargetSource),
    Agent(SshKeyTargetSource),
    Interactive(SshKeyTargetSource),
}
pub(crate) enum Intent {
    Password(PasswordSessionIntent),
    KeyInteractive(
        pulse_app::vault_owner::PrivateKeyInteractiveSessionIntent,
        Arc<super::interactive::Broker>,
    ),
    PrivateKey(PrivateKeySessionIntent),
    Agent(AgentSessionIntent),
    AgentInteractive(
        pulse_app::vault_owner::AgentInteractiveSessionIntent,
        Arc<super::interactive::Broker>,
    ),
    Interactive(InteractiveSessionIntent, Arc<super::interactive::Broker>),
}
impl Gesture {
    pub fn begin_session_authorized(
        self,
        owner: &PageVaultOwner,
        page: &Arc<PageLaunchScope>,
        guard: Guard,
    ) -> std::result::Result<Action, VaultOwnerError> {
        match self {
            Self::Password(v) => v
                .begin_session_authorized(owner, page, guard)
                .map(Action::Password),
            Self::PrivateKey(v) => v
                .begin_session_authorized(owner, page, guard)
                .map(Action::PrivateKey),
            Self::KeyInteractive(v, broker) => v
                .begin_session_authorized(owner, page, guard)
                .map(|v| Action::KeyInteractive(v, broker)),
            Self::Agent(v) => v
                .begin_session_authorized(owner, page, guard)
                .map(Action::Agent),
            Self::AgentInteractive(v, broker) => v
                .begin_session_authorized(owner, page, guard)
                .map(|v| Action::AgentInteractive(v, broker)),
            Self::Interactive(v, broker) => v
                .begin_session_authorized(owner, page, guard)
                .map(|v| Action::Interactive(v, broker)),
        }
    }
}
impl Action {
    pub fn cancellation(&self) -> PageVaultCancellation {
        match self {
            Self::Password(v) => v.cancellation(),
            Self::PrivateKey(v) => v.cancellation(),
            Self::KeyInteractive(v, _) => v.cancellation(),
            Self::Agent(v) => v.cancellation(),
            Self::AgentInteractive(v, _) => v.cancellation(),
            Self::Interactive(v, _) => v.cancellation(),
        }
    }
    pub async fn prepare(
        self,
        store: &StateStore,
        source: Source,
    ) -> std::result::Result<Intent, VaultOwnerError> {
        match (self, source) {
            (Self::Password(v), Source::Password(s)) => {
                v.prepare(store, s).await.map(Intent::Password)
            }
            (Self::PrivateKey(v), Source::PrivateKey(s)) => {
                v.prepare(store, s).await.map(Intent::PrivateKey)
            }
            (Self::KeyInteractive(v, broker), Source::PrivateKey(s)) => v
                .prepare(store, s)
                .await
                .map(|v| Intent::KeyInteractive(v, broker)),
            (Self::Agent(v), Source::Agent(s)) => v.prepare(store, s).await.map(Intent::Agent),
            (Self::AgentInteractive(v, broker), Source::Agent(s)) => v
                .prepare(store, s)
                .await
                .map(|v| Intent::AgentInteractive(v, broker)),
            (Self::Interactive(v, broker), Source::Interactive(s)) => v
                .prepare(store, s)
                .await
                .map(|v| Intent::Interactive(v, broker)),
            _ => Err(VaultOwnerError::Denied),
        }
    }
}
impl Intent {
    pub fn is_temporary(&self) -> bool {
        match self {
            Self::Password(intent) => intent.selected_source().0.is_temporary(),
            Self::PrivateKey(intent) => intent.selected_source().0.is_temporary(),
            Self::KeyInteractive(v, _) => v.selected_source().0.is_temporary(),
            Self::Agent(intent) => intent.selected_source().0.is_temporary(),
            Self::AgentInteractive(v, _) => v.selected_source().0.is_temporary(),
            Self::Interactive(v, _) => v.selected_source().0.is_temporary(),
        }
    }

    pub fn cancellation(&self) -> PageVaultCancellation {
        match self {
            Self::Password(v) => v.cancellation(),
            Self::PrivateKey(v) => v.cancellation(),
            Self::KeyInteractive(v, _) => v.cancellation(),
            Self::Agent(v) => v.cancellation(),
            Self::AgentInteractive(v, _) => v.cancellation(),
            Self::Interactive(v, _) => v.cancellation(),
        }
    }
    pub fn authorization(&self) -> Guard {
        match self {
            Self::Password(v) => v.authorization(),
            Self::PrivateKey(v) => v.authorization(),
            Self::KeyInteractive(v, _) => v.authorization(),
            Self::Agent(v) => v.authorization(),
            Self::AgentInteractive(v, _) => v.authorization(),
            Self::Interactive(v, _) => v.authorization(),
        }
    }
    pub fn deadline(&self) -> std::time::Instant {
        match self {
            Self::Password(v) => v.deadline(),
            Self::PrivateKey(v) => v.deadline(),
            Self::KeyInteractive(v, _) => v.deadline(),
            Self::Agent(v) => v.deadline(),
            Self::AgentInteractive(v, _) => v.deadline(),
            Self::Interactive(v, _) => v.deadline(),
        }
    }
    pub fn dial_target(&self) -> Result<(HostTarget, Vec<HostPin>, (uuid::Uuid, uuid::Uuid))> {
        // Only bounded metadata and parsed trust leave the typed source; no key or credential access.
        macro_rules! target {
            ($v:expr) => {{
                let (source, selected) = $v.selected_source();
                let endpoint = source
                    .hosts()
                    .records()
                    .iter()
                    .find(|row| row.id() == selected.0)
                    .ok_or("workspace.denied")?
                    .endpoint();
                let target = HostTarget::new(endpoint.target().host(), endpoint.target().port())
                    .map_err(|_| "workspace.denied")?;
                let pins = validated_pins(source.trust()).map_err(|_| "workspace.denied")?;
                Ok((target, pins, selected))
            }};
        }
        match self {
            Self::Password(v) => target!(v),
            Self::PrivateKey(v) => target!(v),
            Self::KeyInteractive(v, _) => target!(v),
            Self::Agent(v) => target!(v),
            Self::AgentInteractive(v, _) => target!(v),
            Self::Interactive(v, _) => target!(v),
        }
    }
    pub async fn establish(
        intent: &mut Option<Self>,
        context: &super::starter::StartContext,
        connection: PinnedTransport,
        selected: (uuid::Uuid, uuid::Uuid),
        started: &mut Option<StartedProtocolSession>,
        native: &mut Option<NativeIoCompletion>,
    ) -> Result<ProtocolSessionOwner> {
        // Construct the sink BEFORE taking the original intent/reservation: failed construction must
        // leave their guards in the outer cleanup supervisor until physical I/O has exited.
        enum Sink {
            Password(super::SessionSink),
            PrivateKey(crate::ssh_private_key_adapter::session::SessionSink),
            KeyInteractive(crate::ssh_private_key_adapter::interactive::JointSessionSink),
            Agent(super::agent::SessionSink),
            AgentInteractive(super::agent::interactive::JointSessionSink),
            Interactive(Box<super::interactive::SessionSink>),
        }
        if started.is_none() {
            return Err("workspace.denied");
        }
        let mut sink = match intent.as_ref().ok_or("workspace.denied")? {
            Self::Password(_) => Sink::Password(
                super::SessionSink::new(connection, selected, context.size, context.history_rows)
                    .map_err(|_| "workspace.ssh_failed")?,
            ),
            Self::PrivateKey(_) => Sink::PrivateKey(
                crate::ssh_private_key_adapter::session::SessionSink::new(
                    connection,
                    selected,
                    context.size,
                    context.history_rows,
                )
                .map_err(|_| "workspace.ssh_failed")?,
            ),
            Self::KeyInteractive(_, broker) => Sink::KeyInteractive(
                crate::ssh_private_key_adapter::interactive::JointSessionSink::new(
                    connection,
                    selected,
                    context.size,
                    context.history_rows,
                    broker.clone(),
                )
                .map_err(|_| "workspace.ssh_failed")?,
            ),
            Self::Agent(_) => Sink::Agent(
                super::agent::SessionSink::new(
                    connection,
                    selected,
                    context.size,
                    context.history_rows,
                )
                .await
                .map_err(|clean| {
                    if clean {
                        "workspace.ssh_failed"
                    } else {
                        "workspace.ssh_cleanup_failed"
                    }
                })?,
            ),
            Self::AgentInteractive(v, broker) => Sink::AgentInteractive(
                super::agent::interactive::JointSessionSink::new(
                    connection,
                    selected,
                    context.size,
                    context.history_rows,
                    broker.clone(),
                    context.store.clone(),
                    v.selected_source().0.clone(),
                )
                .await
                .map_err(|clean| {
                    if clean {
                        "workspace.ssh_failed"
                    } else {
                        "workspace.ssh_cleanup_failed"
                    }
                })?,
            ),
            Self::Interactive(v, broker) => Sink::Interactive(Box::new(
                super::interactive::SessionSink::new(
                    connection,
                    selected,
                    context.size,
                    context.history_rows,
                    broker.clone(),
                    context.store.clone(),
                    v.selected_source().0.clone(),
                )
                .map_err(|_| "workspace.ssh_failed")?,
            )),
        };
        match &mut sink {
            Sink::Password(sink) => sink.set_palette(context.palette.clone()),
            Sink::PrivateKey(sink) => sink.set_palette(context.palette.clone()),
            Sink::KeyInteractive(sink) => sink.set_palette(context.palette.clone()),
            Sink::Agent(sink) => sink.set_palette(context.palette.clone()),
            Sink::AgentInteractive(sink) => sink.set_palette(context.palette.clone()),
            Sink::Interactive(sink) => sink.set_palette(context.palette.clone()),
        }
        let taken = intent.take().ok_or("workspace.denied")?;
        let (outcome, alive): (_, Guard) = match (taken, sink) {
            (Self::Password(intent), Sink::Password(sink)) => {
                let admission = sink.observe_admission();
                let alive = Arc::new(move || admission.upgrade().is_some()) as Guard;
                (
                    intent
                        .start(
                            context.store.clone(),
                            sink,
                            started.take().ok_or("workspace.denied")?,
                        )
                        .wait()
                        .await,
                    alive,
                )
            }
            (Self::PrivateKey(intent), Sink::PrivateKey(sink)) => {
                let alive = sink.observe_native_admission();
                (
                    intent
                        .start(
                            context.store.clone(),
                            sink,
                            started.take().ok_or("workspace.denied")?,
                        )
                        .wait()
                        .await,
                    alive,
                )
            }
            (Self::KeyInteractive(intent, _), Sink::KeyInteractive(sink)) => {
                let alive = sink.observe_native_admission();
                (
                    intent
                        .start(
                            context.store.clone(),
                            sink,
                            started.take().ok_or("workspace.denied")?,
                        )
                        .wait()
                        .await,
                    alive,
                )
            }
            (Self::Agent(agent), Sink::Agent(mut sink)) => {
                let alive = sink.observe_native_admission();
                let outcome = agent
                    .authenticate_terminal(&context.store, &mut sink, started)
                    .await;
                let drained = sink.close_and_drain().await;
                drop(sink);
                drop(agent);
                (drained.and(outcome), alive)
            }
            (Self::AgentInteractive(agent, _), Sink::AgentInteractive(mut sink)) => {
                let alive = sink.observe_native_admission();
                let outcome = agent
                    .authenticate_terminal(&context.store, &mut sink, started)
                    .await;
                let drained = sink.close_and_drain().await;
                drop(sink);
                drop(agent);
                (drained.and(outcome), alive)
            }
            (Self::Interactive(interactive, _), Sink::Interactive(mut sink)) => {
                let alive = sink.observe_native_admission();
                let outcome = interactive
                    .authenticate_terminal(&context.store, sink.as_mut(), started)
                    .await;
                let drained = sink.close_and_drain().await;
                drop(sink);
                drop(interactive);
                (drained.and(outcome), alive)
            }
            (taken, _) => {
                *intent = Some(taken);
                return Err("workspace.denied");
            }
        };
        match outcome {
            Ok(owner) => Ok(owner),
            Err(_) => {
                if let Some(io) = native.take() {
                    io.wait()
                        .await
                        .map_err(|_| "workspace.ssh_cleanup_failed")?;
                }
                if alive() {
                    return Err("workspace.ssh_cleanup_failed");
                }
                Err("workspace.ssh_auth_failed")
            }
        }
    }
}
