//! Original-page, frozen-route production driver. No address, credential, or callback comes from IPC.
use super::{SessionSink, secret_target};
#[path = "ssh_route_requests.rs"]
pub(crate) mod requests;
use crate::ssh_private_key_adapter;
use pulse_app::local_session_root::SessionRoot;
use pulse_app::{
    protocol_session_owner::{ProtocolSessionError, ProtocolSessionReservation},
    registry::{PageLaunchScope, SessionId, SessionRegistry},
    vault_owner::{
        PageVaultOwner, PasswordTargetSink, PrivateKeyTargetSink, RouteHopGesture,
        RoutedSessionGesture,
    },
};
use pulse_core::TermSize;
use pulse_ssh::{
    HostTarget,
    managed_store::{FirstPinApproval, FirstPinObservation, validated_pins},
    peer::{PinnedTransport, RouteTransports},
};
use pulse_store::StateStore;
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex},
};
use tokio::io::{AsyncRead, AsyncWrite};
type Result<T> = std::result::Result<T, &'static str>;
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) trait Stream: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Stream for T {}
type OwnedStream = Box<dyn Stream>;
#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Prompt {
    pub index: usize,
    pub count: usize,
    pub entrance: String,
    pub label: String,
    pub host: String,
    pub port: u16,
    pub username: String,
    pub identity: String,
    pub identity_label: String,
    pub algorithm: String,
    pub fingerprint: String,
    pub first_use: bool,
    pub interactive_available: bool,
    pub key_interactive_available: bool,
    pub agent_interactive_available: bool,
}
#[derive(serde::Deserialize)]
#[serde(untagged)]
pub(crate) enum Decision {
    Approved(bool),
    Interactive(InteractiveChoice),
}
#[derive(serde::Deserialize)]
pub(crate) enum InteractiveChoice {
    #[serde(rename = "interactive")]
    Interactive,
    #[serde(rename = "key-interactive")]
    KeyInteractive,
    #[serde(rename = "agent-interactive")]
    AgentInteractive,
}
#[derive(Clone, Copy)]
enum Authentication {
    Saved,
    Interactive,
    KeyInteractive,
    AgentInteractive,
}
impl From<bool> for Decision {
    fn from(value: bool) -> Self {
        Self::Approved(value)
    }
}
impl Decision {
    fn interactive(self) -> Result<Authentication> {
        match self {
            Self::Approved(true) => Ok(Authentication::Saved),
            Self::Interactive(InteractiveChoice::Interactive) => Ok(Authentication::Interactive),
            Self::Interactive(InteractiveChoice::KeyInteractive) => {
                Ok(Authentication::KeyInteractive)
            }
            Self::Interactive(InteractiveChoice::AgentInteractive) => {
                Ok(Authentication::AgentInteractive)
            }
            Self::Approved(false) => Err("workspace.denied"),
        }
    }
}
pub(crate) type Confirm =
    Arc<dyn Fn(Prompt) -> Pin<Box<dyn Future<Output = Result<Decision>> + Send>> + Send + Sync>;
pub(crate) struct Context {
    pub store: StateStore,
    pub registry: SessionRegistry,
    pub page: Arc<PageLaunchScope>,
    pub owner: PageVaultOwner,
    pub root: Arc<Mutex<SessionRoot>>,
    pub size: TermSize,
    pub history_rows: usize,
    pub palette: pulse_app::TerminalStartConfig,
    pub broker: Option<Arc<super::interactive::Broker>>,
}
async fn guarded<T>(
    guard: &Guard,
    deadline: tokio::time::Instant,
    work: impl Future<Output = Result<T>>,
) -> Result<T> {
    if !guard() {
        return Err("workspace.denied");
    }
    // Epoch/page cancellation is rechecked during long network and user-confirmation waits.
    tokio::pin!(work);
    loop {
        tokio::select! { biased;
            _ = tokio::time::sleep_until(deadline) => return Err("workspace.denied"),
            outcome = &mut work => return outcome,
            _ = tokio::time::sleep(std::time::Duration::from_millis(50)) => { if !guard() { return Err("workspace.denied"); } }
        }
    }
}
async fn stream(parents: &mut [PinnedTransport], target: &HostTarget) -> Result<OwnedStream> {
    if let Some(parent) = parents.last_mut() {
        let peer = parent.peer().map_err(|_| "workspace.ssh_peer_failed")?;
        let channel = parent
            .client(&peer)
            .map_err(|_| "workspace.denied")?
            .channel_open_direct_tcpip(target.host(), u32::from(target.port()), "127.0.0.1", 0)
            .await
            .map_err(|_| "workspace.ssh_forward_failed")?;
        Ok(Box::new(channel.into_stream()))
    } else {
        Ok(Box::new(
            tokio::net::TcpStream::connect((target.host(), target.port()))
                .await
                .map_err(|_| "workspace.ssh_dial_failed")?,
        ))
    }
}
async fn drain(parents: &mut Vec<PinnedTransport>) -> Result<()> {
    let mut outcome = Ok(());
    while let Some(parent) = parents.pop() {
        if parent.close_and_join().await.is_err() {
            outcome = Err("workspace.ssh_cleanup_failed");
        }
    }
    outcome
}
pub(crate) async fn run(
    context: Context,
    mut route: RoutedSessionGesture,
    confirm: Confirm,
) -> Result<SessionId> {
    let guard = route.authorization();
    let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(300);
    let hops = route.source().hops().to_vec();
    if hops.len() < 2 || hops.len() > 9 || !guard() {
        return Err("workspace.ssh_route_denied");
    }
    let mut started = Some(
        ProtocolSessionReservation::reserve(&context.registry, &context.page)
            .map_err(|_| "workspace.busy")?
            .begin()
            .map_err(|_| "workspace.denied")?,
    );
    let mut parents = Vec::with_capacity(hops.len());
    let mut cleanup_known = true;
    let outcome = async {
        for (index, hop) in hops.iter().enumerate() {
            let selection = route.take_next_selection().map_err(|_| "workspace.denied")?;
            let mut source = selection.source().clone();
            let target = HostTarget::new(
                hop.endpoint().target().host(),
                hop.endpoint().target().port(),
            )
            .map_err(|_| "workspace.denied")?;
            let identity = match hop.identity() {
                pulse_store::ssh_route_source::SshRouteIdentity::Password(id)
                | pulse_store::ssh_route_source::SshRouteIdentity::PrivateKey(id)
                | pulse_store::ssh_route_source::SshRouteIdentity::Agent(id) => id,
            };
            let identity_label = match hop.identity() {
                pulse_store::ssh_route_source::SshRouteIdentity::Password(id) => source
                    .password_source()
                    .identity_vault()
                    .identities()
                    .records()
                    .find(|row| row.id() == id)
                    .map(|row| row.identity().label().to_owned()),
                pulse_store::ssh_route_source::SshRouteIdentity::PrivateKey(id) => source
                    .identities()
                    .records()
                    .find(|row| row.id() == id)
                    .map(|row| row.identity().label().to_owned()),
                pulse_store::ssh_route_source::SshRouteIdentity::Agent(id) => source
                    .password_source()
                    .agent_identities()
                    .records()
                    .find(|row| row.id() == id)
                    .map(|row| row.identity().label().to_owned()),
            }
            .ok_or("workspace.denied")?;
            let interactive_available =
                source.supports_route_interactive_selection(hop.entrance_id(), identity);
            let key_interactive_available = source.supports_private_key_selection(hop.entrance_id(), identity);
            let agent_interactive_available = source.supports_agent_selection(hop.entrance_id(), identity);
            let mut authentication = Authentication::Saved;
            let make_prompt = |key: &russh::keys::PublicKey, first_use| Prompt {
                interactive_available,
                key_interactive_available,
                agent_interactive_available,
                index,
                count: hops.len(),
                entrance: hop.entrance_id().to_string(),
                label: hop.endpoint().label().into(),
                host: target.host().into(),
                port: target.port(),
                username: hop.endpoint().username().into(),
                identity: identity.to_string(),
                identity_label: identity_label.clone(),
                algorithm: key.algorithm().to_string(),
                fingerprint: key
                    .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
                    .to_string(),
                first_use,
            };
            let known = source.trust().records().iter().any(|pin| {
                pin.target().host() == target.host() && pin.target().port() == target.port()
            });
            if !known {
                let io = guarded(&guard, deadline, stream(&mut parents, &target)).await?;
                // The refused observation joins/destroys its owned stream before returning.
                let observed = FirstPinApproval::inspect(
                    source.password_source().clone(),
                    hop.entrance_id(),
                    Some(vec![]),
                    Arc::new(russh::client::Config::default()),
                    guard.clone(),
                    io,
                )
                .await
                .map_err(|_| "workspace.ssh_inspection_failed")?;
                let FirstPinObservation::FirstUse(prompt) = observed else {
                    return Err("workspace.ssh_fingerprint_changed");
                };
                authentication = guarded(
                    &guard,
                    deadline,
                    confirm(make_prompt(prompt.candidate().public_key(), true)),
                )
                .await?
                .interactive()?;
                let updated = prompt
                    .confirm_source_with_guard(&context.store, true, guard.clone())
                    .await
                    .map_err(|_| "workspace.ssh_pin_failed")?;
                source = route
                    .accept_first_pin(&updated)
                    .map_err(|_| "workspace.denied")?;
            }
            let io = guarded(&guard, deadline, stream(&mut parents, &target)).await?;
            let pending = PinnedTransport::begin_connect_stream(
                target.clone(),
                Some(validated_pins(source.trust()).map_err(|_| "workspace.ssh_source_failed")?),
                Some(vec![]),
                Arc::new(russh::client::Config::default()),
                io,
            )
            .map_err(|_| "workspace.ssh_connect_failed")?;
            let completion = pending.native_io_completion();
            let mut transport = match guarded(&guard, deadline, async {
                pending
                    .connect()
                    .await
                    .map_err(|_| "workspace.ssh_connect_failed")
            })
            .await
            {
                Ok(transport) => transport,
                Err(error) => {
                    if completion.wait().await.is_err() {
                        cleanup_known = false;
                    }
                    return Err(error);
                }
            };
            let peer = match transport.peer() {
                Ok(peer) => peer,
                Err(_) => {
                    cleanup_known &= transport.close_and_join().await.is_ok();
                    return Err("workspace.ssh_peer_failed");
                }
            };
            if known {
                let decision = guarded(
                    &guard,
                    deadline,
                    confirm(make_prompt(peer.public_key(), false)),
                )
                .await;
                if let Ok(choice) = decision.and_then(Decision::interactive) {
                    authentication = choice;
                } else {
                    if transport.close_and_join().await.is_err() {
                        cleanup_known = false;
                    }
                    return Err("workspace.denied");
                }
            }
            let prepared = async {
                let selection = match authentication {
                    Authentication::Saved => selection,
                    Authentication::Interactive if interactive_available && context.broker.is_some() => selection.into_interactive().map_err(|_| "workspace.denied")?,
                    Authentication::KeyInteractive if key_interactive_available && context.broker.is_some() => selection.into_key_interactive().map_err(|_| "workspace.denied")?,
                    Authentication::AgentInteractive if agent_interactive_available && context.broker.is_some() => selection.into_agent_interactive().map_err(|_| "workspace.denied")?,
                    _ => return Err("workspace.denied"),
                };
                let (selection, _) = selection.into_parts();
                Ok(match selection {
                    RouteHopGesture::AgentInteractive(gesture) => Prepared::AgentJoint(Box::new(
                        gesture.begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare_route(&context.store, source.clone()).await.map_err(|_| "workspace.denied")?,
                    )),
                    RouteHopGesture::PrivateKeyInteractive(gesture) => Prepared::Joint(Box::new(
                        gesture.begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare_route(&context.store, source.clone()).await.map_err(|_| "workspace.denied")?,
                    )),
                    RouteHopGesture::Interactive(gesture) => Prepared::Interactive(Box::new(
                        gesture
                            .begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare_route(&context.store, source.clone())
                            .await
                            .map_err(|_| "workspace.denied")?,
                    )),
                    RouteHopGesture::Password(gesture) => Prepared::Password(
                        gesture
                            .begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare(&context.store, source.password_source().clone())
                            .await
                            .map_err(|_| "workspace.denied")?,
                    ),
                    RouteHopGesture::PrivateKey(gesture) => Prepared::Key(
                        gesture
                            .begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare(&context.store, source.clone())
                            .await
                            .map_err(|_| "workspace.denied")?,
                    ),
                    RouteHopGesture::Agent(gesture) => Prepared::Agent(Box::new(
                        gesture
                            .begin_session_authorized(&context.owner, &context.page, guard.clone())
                            .map_err(|_| "workspace.denied")?
                            .prepare(&context.store, source.clone())
                            .await
                            .map_err(|_| "workspace.denied")?,
                    )),
                })
            }
            .await;
            let prepared = match prepared {
                Ok(value) => value,
                Err(error) => {
                    if transport.close_and_join().await.is_err() {
                        cleanup_known = false;
                    }
                    return Err(error);
                }
            };
            let selected = (hop.entrance_id(), identity);
            if index + 1 < hops.len() {
                let result = match prepared {
                    Prepared::AgentJoint(intent) => {
                        let mut sink = match super::agent::forward::joint::JointForwardSink::new(
                            transport, selected, context.size,
                            context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                            context.store.clone(), intent.selected_source().0.clone(),
                        ).await {
                            Ok(sink) => sink,
                            Err(known) => { cleanup_known &= known; return Err("workspace.denied"); }
                        };
                        let result = guarded(&guard, deadline, async {
                            intent.authenticate_forward(&context.store, &mut sink).await.map_err(|_| "workspace.ssh_auth_failed")
                        }).await;
                        if result.is_err() && sink.close_and_drain().await.is_err() { cleanup_known = false; }
                        result
                    }
                    Prepared::Joint(intent) => {
                        let mut sink = match ssh_private_key_adapter::interactive::forward::JointForwardSink::new(
                            transport, selected, context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                        ).await {
                            Ok(sink) => sink,
                            Err(known) => { cleanup_known &= known; return Err("workspace.denied"); }
                        };
                        let result = guarded(&guard, deadline, async {
                            intent.authenticate_forward(&context.store, &mut sink).await.map_err(|_| "workspace.ssh_auth_failed")
                        }).await;
                        if result.is_err() && sink.close_and_drain().await.is_err() { cleanup_known = false; }
                        result
                    }
                    Prepared::Interactive(intent) => {
                        let mut sink = match super::interactive::forward::ForwardSink::new(
                            transport,
                            selected,
                            context.size,
                            context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                            context.store.clone(),
                            intent.selected_source().0.clone(),
                        )
                        .await
                        {
                            Ok(sink) => sink,
                            Err(known) => {
                                cleanup_known &= known;
                                return Err("workspace.denied");
                            }
                        };
                        let result = guarded(&guard, deadline, async {
                            intent
                                .authenticate_forward(&context.store, &mut sink)
                                .await
                                .map_err(|_| "workspace.ssh_auth_failed")
                        })
                        .await;
                        if result.is_err() && sink.close_and_drain().await.is_err() {
                            cleanup_known = false;
                        }
                        result
                    }
                    Prepared::Password(intent) => {
                        let mut sink =
                            match secret_target::forward::ForwardSink::new(transport, selected)
                                .await
                            {
                                Ok(sink) => sink,
                                Err(known) => {
                                    cleanup_known &= known;
                                    return Err("workspace.denied");
                                }
                            };
                        let value = guarded(&guard, deadline, async {
                            intent
                                .authenticate_forward(&context.store, &mut sink)
                                .await
                                .map_err(|_| "workspace.ssh_auth_failed")
                        })
                        .await;
                        if value.is_err() && sink.close_and_drain().await.is_err() {
                            cleanup_known = false;
                        }
                        value
                    }
                    Prepared::Key(intent) => {
                        let mut sink = match ssh_private_key_adapter::forward::ForwardSink::new(
                            transport, selected,
                        )
                        .await
                        {
                            Ok(sink) => sink,
                            Err(known) => {
                                cleanup_known &= known;
                                return Err("workspace.denied");
                            }
                        };
                        let value = guarded(&guard, deadline, async {
                            intent
                                .authenticate_forward(&context.store, &mut sink)
                                .await
                                .map_err(|_| "workspace.ssh_auth_failed")
                        })
                        .await;
                        if value.is_err() && sink.close_and_drain().await.is_err() {
                            cleanup_known = false;
                        }
                        value
                    }
                    Prepared::Agent(intent) => {
                        let mut sink = match super::agent::forward::ForwardSink::new(
                            transport,
                            selected,
                            context.size,
                        )
                        .await
                        {
                            Ok(sink) => sink,
                            Err(known) => {
                                cleanup_known &= known;
                                return Err("workspace.denied");
                            }
                        };
                        let value = guarded(&guard, deadline, async {
                            intent
                                .authenticate_forward(&context.store, &mut sink)
                                .await
                                .map_err(|_| "workspace.ssh_auth_failed")
                        })
                        .await;
                        if value.is_err() && sink.close_and_drain().await.is_err() {
                            cleanup_known = false;
                        }
                        value
                    }
                }?;
                parents.push(result.0);
                // The next strict handshake opens its own exact-target channel from this parent.
                // This replied channel is closed without starting an SDK or sending credentials.
                drop(result.1);
                continue;
            }
            let mut owned = RouteTransports::new(hops.len(), tokio::runtime::Handle::current())
                .map_err(|_| "workspace.denied")?;
            parents.push(transport);
            while !parents.is_empty() {
                let parent = parents.remove(0);
                if let Err(rejected) = owned.push(parent) {
                    let (_, transport) = rejected.into_parts();
                    if transport.close_and_join().await.is_err() {
                        cleanup_known = false;
                    }
                    if owned.begin_close().wait().await.is_err() {
                        cleanup_known = false;
                    }
                    return Err("workspace.denied");
                }
            }
            transport = match owned.take_final() {
                Ok(final_transport) => final_transport,
                Err(_) => {
                    cleanup_known &= owned.begin_close().wait().await.is_ok();
                    return Err("workspace.denied");
                }
            };
            let owner = match prepared {
                Prepared::AgentJoint(intent) => {
                    let mut sink = match super::agent::interactive::JointSessionSink::new(
                        transport, selected, context.size, context.history_rows,
                        context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                        context.store.clone(), intent.selected_source().0.clone(),
                    ).await {
                        Ok(sink) => sink,
                        Err(known) => { cleanup_known &= known; cleanup_known &= owned.begin_close().wait().await.is_ok(); return Err("workspace.denied"); }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent.authenticate_route_terminal(&context.store, &mut sink, &mut started).await.map_err(|_| "workspace.ssh_auth_failed")
                    }).await;
                    if result.is_err() && sink.close_and_drain().await.is_err() { cleanup_known = false; }
                    result
                }
                Prepared::Joint(intent) => {
                    let mut sink = match ssh_private_key_adapter::interactive::JointSessionSink::new_route(
                        transport, selected, context.size, context.history_rows,
                        context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                    ).await {
                        Ok(sink) => sink,
                        Err(known) => { cleanup_known &= known; cleanup_known &= owned.begin_close().wait().await.is_ok(); return Err("workspace.denied"); }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent.authenticate_route_terminal(&context.store, &mut sink, &mut started).await.map_err(|_| "workspace.ssh_auth_failed")
                    }).await;
                    if result.is_err() && sink.close_and_drain().await.is_err() { cleanup_known = false; }
                    result
                }
                Prepared::Interactive(intent) => {
                    let mut sink = match super::interactive::SessionSink::new_route(
                        transport,
                        selected,
                        context.size,
                        context.history_rows,
                        context.broker.as_ref().ok_or("workspace.denied")?.clone(),
                        context.store.clone(),
                        intent.selected_source().0.clone(),
                    )
                    .await
                    {
                        Ok(sink) => sink,
                        Err(known) => {
                            cleanup_known &= known;
                            cleanup_known &= owned.begin_close().wait().await.is_ok();
                            return Err("workspace.denied");
                        }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent
                            .authenticate_terminal(&context.store, &mut sink, &mut started)
                            .await
                            .map_err(|_| "workspace.ssh_auth_failed")
                    })
                    .await;
                    if result.is_err() && sink.close_and_drain().await.is_err() {
                        cleanup_known = false;
                    }
                    result
                }
                Prepared::Password(intent) => {
                    let mut sink = match SessionSink::new_route(
                        transport,
                        selected,
                        context.size,
                        context.history_rows,
                    )
                    .await
                    {
                        Ok(sink) => sink,
                        Err(known) => {
                            cleanup_known &= known;
                            cleanup_known &= owned.begin_close().wait().await.is_ok();
                            return Err("workspace.denied");
                        }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent
                            .authenticate_route_terminal(&context.store, &mut sink, &mut started)
                            .await
                            .map_err(|_| "workspace.ssh_auth_failed")
                    })
                    .await;
                    if result.is_err() && sink.close_and_drain().await.is_err() {
                        cleanup_known = false;
                    }
                    result
                }
                Prepared::Key(intent) => {
                    let mut sink = match ssh_private_key_adapter::session::SessionSink::new_route(
                        transport,
                        selected,
                        context.size,
                        context.history_rows,
                    )
                    .await
                    {
                        Ok(sink) => sink,
                        Err(known) => {
                            cleanup_known &= known;
                            cleanup_known &= owned.begin_close().wait().await.is_ok();
                            return Err("workspace.denied");
                        }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent
                            .authenticate_route_terminal(&context.store, &mut sink, &mut started)
                            .await
                            .map_err(|_| "workspace.ssh_auth_failed")
                    })
                    .await;
                    if result.is_err() && sink.close_and_drain().await.is_err() {
                        cleanup_known = false;
                    }
                    result
                }
                Prepared::Agent(intent) => {
                    let mut sink = match super::agent::SessionSink::new(
                        transport,
                        selected,
                        context.size,
                        context.history_rows,
                    )
                    .await
                    {
                        Ok(sink) => sink,
                        Err(known) => {
                            cleanup_known &= known;
                            cleanup_known &= owned.begin_close().wait().await.is_ok();
                            return Err("workspace.denied");
                        }
                    };
                    sink.set_palette(context.palette.clone());
                    sink.attach_parents(owned);
                    let result = guarded(&guard, deadline, async {
                        intent
                            .authenticate_terminal(&context.store, &mut sink, &mut started)
                            .await
                            .map_err(|_| "workspace.ssh_auth_failed")
                    })
                    .await;
                    if result.is_err() && sink.close_and_drain().await.is_err() {
                        cleanup_known = false;
                    }
                    result
                }
            }?;
            let session = context
                .root
                .lock()
                .map_err(|_| "workspace.ssh_failed")?
                .adopt_protocol(owner)
                .map_err(|_| "workspace.denied")?;
            if context
                .store
                .record_recent_ssh_host(hop.entrance_id())
                .await
                .is_err()
            {
                eprintln!("workspace.recent_host_record_failed");
            }
            eprintln!("workspace.ssh_session_opened");
            return Ok(session);
        }
        Err("workspace.ssh_route_failed")
    }
    .await;
    if drain(&mut parents).await.is_err() {
        cleanup_known = false;
    }
    if let Some(started) = started.take() {
        let cleaned = started
            .finish_unregistered(async move {
                if cleanup_known {
                    Ok(())
                } else {
                    Err(ProtocolSessionError::NativeUnknown)
                }
            })
            .join()
            .await;
        if cleaned.is_err() {
            return Err("workspace.ssh_cleanup_failed");
        }
    }
    drop(route);
    outcome
}
enum Prepared {
    AgentJoint(Box<pulse_app::vault_owner::AgentInteractiveSessionIntent>),
    Joint(Box<pulse_app::vault_owner::PrivateKeyInteractiveSessionIntent>),
    Interactive(Box<pulse_app::vault_owner::InteractiveSessionIntent>),
    Agent(Box<pulse_app::vault_owner::AgentSessionIntent>),
    Password(pulse_app::vault_owner::PasswordSessionIntent),
    Key(pulse_app::vault_owner::PrivateKeySessionIntent),
}
