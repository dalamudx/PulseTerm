//! Saved-source SSH CLI. A socket hello is not GUI, key, peer or target authorization.
use super::{Notice, Workspace, approvals, event, on_original_page, original_current};
use pulse_app::vault_owner::SavedSessionSelection;
use pulse_app::{cli_control, registry::PageLaunchScope, window_action::PreparedWindowAction};
use serde_json::{Value, json};
use std::sync::Arc;
use tauri::{AppHandle, Manager};
use tokio_util::sync::CancellationToken;
use uuid::Uuid;
type Result<T> = cli_control::Result<T>;
pub(super) struct StartRequest<'a> {
    pub params: &'a serde_json::Map<String, Value>,
    pub cold_startup: bool,
}
#[path = "product_cli_cold_ssh.rs"]
mod cold;
#[path = "product_cli_ssh_interactive.rs"]
mod mfa;
#[path = "product_cli_ssh_route.rs"]
mod route;
enum DirectSelection {
    Saved(SavedSessionSelection),
    Interactive(mfa::Gesture),
}
enum PreparedSelection {
    Direct(SavedSessionSelection),
    Interactive(mfa::Gesture),
    Route(
        pulse_app::vault_owner::SavedSessionGesture,
        Box<pulse_store::ssh_route_source::SshRouteSource>,
    ),
}
// Source resolution creates metadata for a separate precise-target click. It cannot
// upgrade the alias invitation into confirmed host/identity or actual-peer authority.
pub(super) fn resolved_alias_action(
    registry: &pulse_app::registry::SessionRegistry,
    current: Arc<PageLaunchScope>,
    host: Uuid,
    identity: Uuid,
) -> Result<PreparedWindowAction> {
    let catalogue = super::capabilities::catalogue(true).map_err(|_| cli_control::Error)?;
    PreparedWindowAction::prepare(
        &catalogue,
        registry,
        current,
        "workspace.ssh",
        pulse_cap::Surface::Cli,
        json!({"host":host.to_string(), "identity":identity.to_string()}),
    )
    .map_err(|_| cli_control::Error)
}
enum CapturedSelection {
    Saved(Uuid, Uuid, pulse_app::vault_owner::SavedSessionGesture),
    Alias(pulse_app::vault_owner::SavedAliasGesture),
}
fn saved(value: Option<&Value>) -> Result<Uuid> {
    let text = value.and_then(Value::as_str).ok_or(cli_control::Error)?;
    let id = Uuid::parse_str(text).map_err(|_| cli_control::Error)?;
    if id.get_version_num() != 7
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != text
    {
        return Err(cli_control::Error);
    }
    Ok(id)
}
struct CancelStart {
    requests: Arc<crate::ssh_session_adapter::requests::SessionRequests>,
    page: Arc<PageLaunchScope>,
    id: Uuid,
}
impl Drop for CancelStart {
    fn drop(&mut self) {
        let _cancel = self.requests.cancel(&self.page, self.id);
    }
}
pub(super) async fn run(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    approvals: Arc<approvals::Approvals>,
    action: PreparedWindowAction,
    request: StartRequest<'_>,
) -> Result<Value> {
    let StartRequest {
        params,
        cold_startup,
    } = request;
    crate::cli_startup::StartupAction::new("workspace.ssh", &Value::Object(params.clone()))
        .map_err(|_| cli_control::Error)?;
    let alias = params
        .get("alias")
        .and_then(Value::as_str)
        .map(str::to_owned);
    let authentication = match params.get("authentication") {
        None => None,
        Some(Value::String(method)) if method == "interactive" => Some("interactive"),
        Some(Value::String(method)) if method == "key-interactive" => Some("key-interactive"),
        Some(Value::String(method)) if method == "agent-interactive" => Some("agent-interactive"),
        _ => return Err(cli_control::Error),
    };
    let interactive = authentication == Some("interactive");
    let key_interactive = authentication == Some("key-interactive");
    let agent_interactive = authentication == Some("agent-interactive");
    let root = Arc::downgrade(&state.0);
    let owner = state.0.owner;
    let page = original.clone();
    let stop = cancelled.clone();
    let authorization: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        root.upgrade()
            .is_some_and(|state| original_current(&state.documents, owner, &page, &stop).is_ok())
    });
    let vault = state.0.vault.clone();
    let guard = authorization.clone();
    // No native startup/CPU reservation or 5s connection timer while waiting for the GUI.
    let direct_deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
    let capture = if let Some(alias) = alias {
        let gesture = if cold_startup {
            cold::confirm_alias(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                approvals.clone(),
                alias,
                authorization.clone(),
            )
            .await?
        } else {
            on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |_, current| {
                    vault
                        .begin_saved_alias_gesture_authorized(current, &alias, guard)
                        .map_err(|_| cli_control::Error)
                },
            )
            .await?
        };
        CapturedSelection::Alias(gesture)
    } else {
        let host = saved(params.get("host"))?;
        let identity = saved(params.get("identity"))?;
        let gesture = if cold_startup {
            cold::confirm_intent(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                approvals.clone(),
                (host, identity, authentication),
                authorization.clone(),
            )
            .await?
        } else {
            on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |_, current| {
                    vault
                        .begin_routed_session_gesture_authorized(current, host, identity, guard)
                        .map_err(|_| cli_control::Error)
                },
            )
            .await?
        };
        CapturedSelection::Saved(host, identity, gesture)
    };
    let directory = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, _| {
            window
                .app_handle()
                .path()
                .app_data_dir()
                .map_err(|_| cli_control::Error)
        },
    )
    .await?;
    let (store, source) = state
        .0
        .profiles
        .ssh_key_source(
            state.0.registry.clone(),
            original.clone(),
            directory,
            authorization,
            None,
        )
        .await
        .map_err(|_| cli_control::Error)?;
    let resolved_alias = matches!(&capture, CapturedSelection::Alias(_));
    let (host, identity, gesture) = match capture {
        CapturedSelection::Saved(host, identity, gesture) => (host, identity, gesture),
        CapturedSelection::Alias(gesture) => {
            let (gesture, selection) = gesture
                .select_source(&store, &source)
                .map_err(|_| cli_control::Error)?;
            (selection.host_id(), selection.identity_id(), gesture)
        }
    };
    let routed = source.hosts().catalogue().and_then(|graph| {
        graph
            .profiles()
            .iter()
            .find(|profile| {
                profile.endpoint_id() == host
                    && !profile.metadata().deleted()
                    && profile.jump_profile_id().is_some()
            })
            .map(|profile| profile.metadata().id())
    });
    let (selection, private_key, agent) = if key_interactive {
        if routed.is_some()
            || resolved_alias
            || !source.supports_private_key_selection(host, identity)
        {
            return Err(cli_control::Error);
        }
        (
            PreparedSelection::Interactive(mfa::Gesture::PrivateKey(
                gesture
                    .into_key_interactive()
                    .map_err(|_| cli_control::Error)?,
            )),
            true,
            false,
        )
    } else if agent_interactive {
        if routed.is_some() || resolved_alias || !source.supports_agent_selection(host, identity) {
            return Err(cli_control::Error);
        }
        (
            PreparedSelection::Interactive(mfa::Gesture::Agent(
                gesture
                    .into_agent_interactive()
                    .map_err(|_| cli_control::Error)?,
            )),
            false,
            true,
        )
    } else if interactive {
        if routed.is_some()
            || resolved_alias
            || !source.supports_interactive_selection(host, identity)
        {
            return Err(cli_control::Error);
        }
        (
            PreparedSelection::Interactive(mfa::Gesture::Interactive(
                gesture.into_interactive().map_err(|_| cli_control::Error)?,
            )),
            false,
            false,
        )
    } else if let Some(profile) = routed {
        use pulse_store::ssh_route_source::SshRouteIdentity;
        let password = source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .any(|r| r.id() == identity);
        let private_key = source.identities().records().any(|r| r.id() == identity);
        let agent = source
            .password_source()
            .agent_identities()
            .records()
            .any(|r| r.id() == identity);
        if usize::from(password) + usize::from(private_key) + usize::from(agent) != 1 {
            return Err(cli_control::Error);
        }
        let kind = if agent {
            SshRouteIdentity::Agent(identity)
        } else if private_key {
            SshRouteIdentity::PrivateKey(identity)
        } else {
            SshRouteIdentity::Password(identity)
        };
        let route = store
            .resolve_ssh_route_source(source.clone(), profile, kind)
            .map_err(|_| cli_control::Error)?;
        (
            PreparedSelection::Route(gesture, Box::new(route)),
            private_key,
            agent,
        )
    } else {
        let selected = gesture
            .tighten_deadline(direct_deadline)
            .select_source(&source)
            .map_err(|_| cli_control::Error)?;
        let private_key = matches!(&selected, SavedSessionSelection::PrivateKey(_));
        let agent = matches!(&selected, SavedSessionSelection::Agent(_));
        (PreparedSelection::Direct(selected), private_key, agent)
    };
    let endpoint = match &selection {
        PreparedSelection::Route(_, route) => {
            route.hops().last().ok_or(cli_control::Error)?.endpoint()
        }
        PreparedSelection::Direct(_) | PreparedSelection::Interactive(_) => source
            .hosts()
            .records()
            .iter()
            .find(|row| row.id() == host)
            .ok_or(cli_control::Error)?
            .endpoint(),
    };
    let (identity_label, username) = if private_key {
        let value = source
            .identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(cli_control::Error)?
            .identity();
        (
            format!("私钥：{}", value.label()),
            value.username().to_owned(),
        )
    } else if agent {
        let value = source
            .password_source()
            .agent_identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(cli_control::Error)?
            .identity();
        (
            format!("Agent：{}", value.label()),
            value.username().to_owned(),
        )
    } else {
        let value = source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .find(|row| row.id() == identity)
            .ok_or(cli_control::Error)?
            .identity();
        (value.label().to_owned(), value.username().to_owned())
    };
    let prompt = json!({"hostId":host.to_string(),"identityId":identity.to_string(),"hostLabel":endpoint.label(),"address":endpoint.target().host(),"port":endpoint.target().port(),"identityLabel":identity_label,"username":username});
    let stop = cancelled.clone();
    let registry = state.0.registry.clone();
    let mut reservation = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, current| {
            let action = if resolved_alias {
                resolved_alias_action(&registry, current.clone(), host, identity)?
            } else {
                action
            };
            let reservation = approvals
                .reserve(current, stop, action)
                .map_err(|_| cli_control::Error)?;
            event(
                &window,
                "pulseterm:cli-approval",
                json!({"version":2,"ticket":reservation.id.to_string(),"ssh":if let Some(authentication) = authentication {
                    let mut prompt = prompt; prompt["authentication"] = json!(authentication); prompt
                } else { prompt }}),
            )?;
            eprintln!("workspace.cli_ssh_approval_requested");
            Ok(reservation)
        },
    )
    .await?;
    let ticket = reservation.id;
    let _notice = Notice {
        app: app.clone(),
        id: ticket,
        window_label: state
            .native_label()
            .map_err(|_| cli_control::Error)?
            .to_owned(),
    };
    let action = tokio::select! {biased;
        _=cancelled.cancelled()=>return Err(cli_control::Error),
        result=tokio::time::timeout(approvals::WAIT,&mut reservation.answer)=>result.map_err(|_|cli_control::Error)?.map_err(|_|cli_control::Error)?.map_err(|_|cli_control::Error)?,
    };
    drop(reservation);
    let selection = match selection {
        PreparedSelection::Direct(selected) => DirectSelection::Saved(selected),
        PreparedSelection::Interactive(gesture) => DirectSelection::Interactive(gesture),
        PreparedSelection::Route(captured, route) => {
            on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |_, current| {
                    let args = action
                        .take_args(&current, "workspace.ssh")
                        .map_err(|_| cli_control::Error)?;
                    if args != json!({"host":host.to_string(), "identity":identity.to_string()}) {
                        return Err(cli_control::Error);
                    }
                    Ok(())
                },
            )
            .await?;
            return route::run(app, state, original, cancelled, store, captured, *route).await;
        }
    };
    let requests = state.0.ssh_sessions.native_requests();
    let bank = requests.clone();
    let vault = state.0.vault.clone();
    let _cancel = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, current| {
            let args = action
                .take_args(&current, "workspace.ssh")
                .map_err(|_| cli_control::Error)?;
            let mut expected = json!({"host":host.to_string(),"identity":identity.to_string()});
            if let Some(authentication) = authentication {
                expected["authentication"] = json!(authentication);
            }
            if args != expected {
                return Err(cli_control::Error);
            }
            let id = match selection {
                DirectSelection::Saved(SavedSessionSelection::Password(gesture)) => {
                    bank.begin_gesture(current.clone(), host, identity, &vault, gesture)
                }
                DirectSelection::Saved(SavedSessionSelection::PrivateKey(gesture)) => {
                    bank.begin_key_gesture(current.clone(), host, identity, &vault, gesture)
                }
                DirectSelection::Saved(SavedSessionSelection::Agent(gesture)) => {
                    bank.begin_agent_gesture(current.clone(), host, identity, &vault, gesture)
                }
                DirectSelection::Interactive(gesture) => mfa::begin(
                    window,
                    current.clone(),
                    &bank,
                    &vault,
                    (host, identity),
                    gesture,
                    ticket,
                ),
            }
            .map_err(|_| cli_control::Error)?;
            Ok(CancelStart {
                requests: bank.clone(),
                page: current,
                id,
            })
        },
    )
    .await?;
    // Lost waiters revoke only this request. Bank-owned native work retains its permit until exit.
    let id = _cancel.id;
    let startup_cancelled = if authentication.is_some() {
        Some(
            requests
                .cancellation(&original, id)
                .map_err(|_| cli_control::Error)?,
        )
    } else {
        None
    };
    if interactive {
        requests
            .prepare_interactive(original.clone(), id, async move { Ok((store, source)) })
            .await
            .map_err(|_| cli_control::Error)?;
    } else if private_key {
        requests
            .prepare_key(original.clone(), id, async move { Ok((store, source)) })
            .await
            .map_err(|_| cli_control::Error)?;
    } else if agent {
        requests
            .prepare_agent(original.clone(), id, async move { Ok((store, source)) })
            .await
            .map_err(|_| cli_control::Error)?;
    } else {
        let source = source.password_source().clone();
        requests
            .prepare(original.clone(), id, async move { Ok((store, source)) })
            .await
            .map_err(|_| cli_control::Error)?;
    }
    let target = crate::ssh_session_adapter::requests::StartTarget {
        root: state.0.sessions.clone(),
        size: pulse_core::TermSize::new(80, 24).map_err(|_| cli_control::Error)?,
        history_rows: 10000,
        palette: super::super::ssh_connection::startup_palette(
            &app,
            &state,
            original.clone(),
            Some(host),
        )
        .await
        .map_err(|_| cli_control::Error)?,
    };
    let starting = requests.start(original.clone(), id, target);
    let outcome = if let Some(stop) = startup_cancelled {
        // Dropping this waiter leaves the bank-owned native task and permit alive.
        tokio::select! {
            biased;
            result = starting => result,
            _ = stop.cancelled() => Err("workspace.denied"),
            _ = cancelled.cancelled() => Err("workspace.denied"),
        }
    } else {
        starting.await
    };
    let session = outcome.map_err(|_| cli_control::Error)?.get().to_string();
    // Native root handoff is never rolled back because a page/event/CLI response disappeared.
    let rendered = session.clone();
    on_original_page(app, state, original, cancelled, move |window, _| {
        event(
            &window,
            "pulseterm:cli-session",
            json!({"ticket":ticket.to_string(),"session":rendered}),
        )?;
        eprintln!("workspace.cli_ssh_opened");
        Ok(())
    })
    .await?;
    Ok(json!({"session":session}))
}
