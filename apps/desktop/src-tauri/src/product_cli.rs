//! Opt-in Linux warm actions. Hello is not execution or GUI approval authority.
use super::{Workspace, capabilities, local_workspace};
use pulse_app::{
    cli_control::{self, Handler, Operation, Server},
    registry::PageLaunchScope,
    window_action::PreparedWindowAction,
};
use pulse_cap::Surface;
use pulse_ipc::cli::Request;
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};
use tauri::{AppHandle, Manager, WebviewWindow};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
#[path = "product_cli_approvals.rs"]
mod approvals;
#[path = "product_cli_new_window.rs"]
mod new_window;
pub(super) use approvals::Approvals;
#[path = "cli_profiles.rs"]
mod profiles;
#[cfg(feature = "workspace-ssh")]
#[path = "product_cli_ssh.rs"]
mod ssh;
#[cfg(test)]
#[path = "product_cli_tests.rs"]
mod tests;

fn original_current(
    documents: &pulse_app::product_document::ProductDocuments,
    owner: pulse_app::registry::WindowToken,
    original: &Arc<PageLaunchScope>,
    cancelled: &CancellationToken,
) -> cli_control::Result<Arc<PageLaunchScope>> {
    let current = documents
        .current_native(owner)
        .map_err(|_| cli_control::Error)?;
    if !Arc::ptr_eq(original, &current) || cancelled.is_cancelled() {
        return Err(cli_control::Error);
    }
    Ok(current)
}
pub(super) fn answer(
    state: &Workspace,
    page: &Arc<PageLaunchScope>,
    ticket: &str,
    accept: bool,
) -> super::Result<()> {
    let ticket = uuid::Uuid::parse_str(ticket).map_err(|_| "workspace.cli_forbidden")?;
    state.0.cli_approvals.answer(page, ticket, accept)
}
pub(super) fn retire(state: &Workspace) {
    state.0.cli_approvals.retire();
}
#[derive(Clone)]
struct Actions {
    app: AppHandle,
    state: Workspace,
    approvals: Arc<approvals::Approvals>,
}
/// GUI checks precede effects and follow asynchronous reads. New pages cannot substitute original.
async fn on_original_page<T: Send + 'static>(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    effect: impl FnOnce(WebviewWindow, Arc<PageLaunchScope>) -> cli_control::Result<T> + Send + 'static,
) -> cli_control::Result<T> {
    let (tx, rx) = oneshot::channel();
    let gui = app.clone();
    app.run_on_main_thread(move || {
        let outcome = (|| {
            if cancelled.is_cancelled() || tx.is_closed() {
                return Err(cli_control::Error);
            }
            let label = state.native_label().map_err(|_| cli_control::Error)?;
            let window = gui.get_webview_window(label).ok_or(cli_control::Error)?;
            if !local_workspace(&window) {
                return Err(cli_control::Error);
            }
            let current =
                original_current(&state.0.documents, state.0.owner, &original, &cancelled)?;
            effect(window, current)
        })();
        let _ = tx.send(outcome);
    })
    .map_err(|_| cli_control::Error)?;
    rx.await.map_err(|_| cli_control::Error)?
}
fn source(
    window: &WebviewWindow,
    kind: pulse_pty::profiles::ShellKind,
) -> cli_control::Result<super::profiles::ProfileStart> {
    Ok(super::profiles::ProfileStart {
        preset: pulse_store::settings::TerminalPreset::Classic.into(),
        directory: window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| cli_control::Error)?,
        path: std::env::var_os("PATH").ok_or(cli_control::Error)?,
        shell: std::env::var_os("SHELL").map(std::path::PathBuf::from),
        kind,
    })
}
fn event(window: &WebviewWindow, name: &'static str, value: Value) -> cli_control::Result<()> {
    // Only backend fixed event names, known enums/UUIDs/session IDs. Never client code or source text.
    let body = serde_json::to_string(&value).map_err(|_| cli_control::Error)?;
    window
        .eval(format!(
            "window.dispatchEvent(new CustomEvent('{name}',{{detail:{body}}}));"
        ))
        .map_err(|_| cli_control::Error)
}

fn owned_selection(state: &Workspace, session: &str) -> cli_control::Result<u64> {
    state
        .0
        .sessions
        .lock()
        .map_err(|_| cli_control::Error)?
        .ids()
        .into_iter()
        .find(|id| id.get().to_string() == session)
        .map(|id| id.get())
        .ok_or(cli_control::Error)
}
async fn select_tab(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    action: PreparedWindowAction,
) -> cli_control::Result<Value> {
    select_session(app, state, original, cancelled, move |current| {
        let args = action
            .take_args(current, "workspace.focus-tab")
            .map_err(|_| cli_control::Error)?;
        Ok(args
            .get("session")
            .and_then(Value::as_str)
            .ok_or(cli_control::Error)?
            .to_owned())
    })
    .await
}
#[path = "product_cli_local_presentation.rs"]
mod local_presentation;
#[path = "product_cli_windows.rs"]
mod windows;
// Only trusted native startup code supplies its already registered typed session ID.
async fn present_registered_session(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    session: pulse_app::registry::SessionId,
) -> cli_control::Result<Value> {
    select_session(app, state, original, cancelled, move |_| {
        Ok(session.get().to_string())
    })
    .await
}
async fn select_session(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    resolve: impl FnOnce(&Arc<PageLaunchScope>) -> cli_control::Result<String> + Send + 'static,
) -> cli_control::Result<Value> {
    let root = state.clone();
    let stop = cancelled.clone();
    let notice_app = app.clone();
    let (mut reservation, session, _notice) = on_original_page(app.clone(), state.clone(), original.clone(), cancelled.clone(), move |window, current| {
        let target = resolve(&current)?;
        let session = owned_selection(&root, &target)?;
        let (nonce, exact) = root.0.documents.current_native_document(root.0.owner).map_err(|_| cli_control::Error)?;
        if !Arc::ptr_eq(&current, &exact) { return Err(cli_control::Error); }
        let reservation = root.0.selections.begin(current, session, stop).map_err(|_| cli_control::Error)?;
        let notice = Notice { app: notice_app, id: reservation.id, window_label: root.native_label().map_err(|_| cli_control::Error)?.to_owned() };
        event(&window, "pulseterm:cli-focus-tab", json!({"version":1,"nonce":nonce.to_string(),"ticket":reservation.id.to_string(),"session":session.to_string()}))?;
        Ok((reservation, session, notice))
    }).await?;
    tokio::select! { biased;
        _ = cancelled.cancelled() => return Err(cli_control::Error),
        result = tokio::time::timeout(Duration::from_secs(5), &mut reservation.answer) => result.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error)?,
    }
    let root = state.clone();
    on_original_page(app, state, original, cancelled, move |window, _| {
        owned_selection(&root, &session.to_string())?;
        window.set_focus().map_err(|_| cli_control::Error)?;
        Ok(json!({"session":session.to_string(),"selectionConfirmed":true}))
    })
    .await
}
struct Notice {
    app: AppHandle,
    id: uuid::Uuid,
    window_label: String,
}
impl Drop for Notice {
    fn drop(&mut self) {
        let app = self.app.clone();
        let id = self.id;
        let window_label = self.window_label.clone();
        let gui = app.clone();
        let _ = app.run_on_main_thread(move || {
            if let Some(window) = gui.get_webview_window(&window_label)
                && local_workspace(&window)
            {
                let _ = event(
                    &window,
                    "pulseterm:cli-cancel",
                    json!({"ticket":id.to_string()}),
                );
            }
        });
    }
}
// These phases are constructed only by native bootstrap code, never socket parameters.
#[derive(Clone)]
enum CallOrigin {
    Routed(Arc<PageLaunchScope>),
    Startup(Arc<PageLaunchScope>),
    SplitBase {
        page: Arc<PageLaunchScope>,
        direction: String,
    },
    SplitFollowup {
        page: Arc<PageLaunchScope>,
        target: pulse_app::registry::SessionId,
    },
}
#[path = "product_cli_cold_split.rs"]
mod cold_split;
impl Actions {
    fn call_scoped(
        &self,
        request: Request,
        origin: CallOrigin,
        cancelled: CancellationToken,
    ) -> Operation {
        let app = self.app.clone();
        let state = self.state.clone();
        let approvals = self.approvals.clone();
        let registration_only = matches!(&origin, CallOrigin::SplitBase { .. });
        let actions = self.clone();
        Box::pin(async move {
            if !matches!(
                request.method(),
                "workspace.focus"
                    | "workspace.profiles"
                    | "workspace.focus-tab"
                    | "workspace.new-tab"
                    | "workspace.new-window"
                    | "workspace.split"
                    | "workspace.ssh"
            ) || cancelled.is_cancelled()
            {
                return Err(cli_control::Error);
            }
            let catalogue = capabilities::catalogue(cfg!(feature = "workspace-ssh"))
                .map_err(|_| cli_control::Error)?;
            // Only trusted bootstrap passes a captured original scope. Socket requests cannot select this phase.
            let cold_startup = matches!(&origin, CallOrigin::Startup(_));
            let cold_split_base = match &origin {
                CallOrigin::SplitBase { direction, .. } => Some(direction.clone()),
                _ => None,
            };
            let cold_split_target = match &origin {
                CallOrigin::SplitFollowup { target, .. } => Some(target.get().to_string()),
                _ => None,
            };
            let captured = match origin {
                CallOrigin::Routed(page)
                | CallOrigin::Startup(page)
                | CallOrigin::SplitBase { page, .. }
                | CallOrigin::SplitFollowup { page, .. } => page,
            };
            let original =
                original_current(&state.0.documents, state.0.owner, &captured, &cancelled)?;
            if cold_startup && request.method() == "workspace.split" {
                return cold_split::run(actions, request, original, cancelled).await;
            }
            let action = PreparedWindowAction::prepare(
                &catalogue,
                &state.0.registry,
                original.clone(),
                request.method(),
                Surface::Cli,
                Value::Object(request.params().clone()),
            )
            .map_err(|_| cli_control::Error)?;
            if request.method() == "workspace.new-window" {
                return new_window::run(app, state, original, cancelled, action).await;
            }
            if request.method() == "workspace.ssh" {
                #[cfg(feature = "workspace-ssh")]
                return ssh::run(
                    app,
                    state,
                    original,
                    cancelled,
                    approvals,
                    action,
                    ssh::StartRequest {
                        params: request.params(),
                        cold_startup,
                    },
                )
                .await;
                #[cfg(not(feature = "workspace-ssh"))]
                return Err(cli_control::Error);
            }
            if request.method() == "workspace.focus-tab" {
                return select_tab(app, state, original, cancelled, action).await;
            }
            if request.method() == "workspace.focus" {
                let stop = cancelled.clone();
                return on_original_page(
                    app,
                    state,
                    original,
                    cancelled,
                    move |window, current| {
                        action
                            .take_args(&current, "workspace.focus")
                            .map_err(|_| cli_control::Error)?;
                        if stop.is_cancelled() {
                            return Err(cli_control::Error);
                        }
                        window.set_focus().map_err(|_| cli_control::Error)?;
                        Ok(json!({"focusRequested":true}))
                    },
                )
                .await;
            }
            let selected = if matches!(request.method(), "workspace.new-tab" | "workspace.split") {
                Some(
                    request
                        .params()
                        .get("profile")
                        .and_then(Value::as_str)
                        .ok_or(cli_control::Error)?
                        .to_owned(),
                )
            } else {
                None
            };
            let split = request.method() == "workspace.split";
            let launch_action = if split {
                "workspace.split"
            } else {
                "workspace.new-tab"
            };
            let direction = if split {
                Some(
                    request
                        .params()
                        .get("direction")
                        .and_then(Value::as_str)
                        .ok_or(cli_control::Error)?
                        .to_owned(),
                )
            } else {
                None
            };
            let launching = selected.is_some();
            let (inputs, action) = on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |window, current| {
                    let action = if launching {
                        Some(action)
                    } else {
                        action
                            .take_args(&current, "workspace.profiles")
                            .map_err(|_| cli_control::Error)?;
                        None
                    };
                    Ok((source(&window, pulse_pty::profiles::ShellKind::Sh)?, action))
                },
            )
            .await?;
            // Supervised I/O credit belongs to the blocking/SQL operation until actual exit.
            let offers = state
                .0
                .profiles
                .list(
                    state.0.registry.clone(),
                    original.clone(),
                    inputs.directory.clone(),
                    inputs.path.clone(),
                    inputs.shell.clone(),
                )
                .await
                .map_err(|_| cli_control::Error)?;
            let custom = state
                .0
                .profiles
                .custom_list(
                    state.0.registry.clone(),
                    original.clone(),
                    (inputs.directory, inputs.path, inputs.shell),
                )
                .await
                .map_err(|_| cli_control::Error)?;
            let custom_ids: Vec<_> = custom
                .iter()
                .map(|profile| profile.id.to_string())
                .collect();
            let value = profiles::project(
                offers.iter().map(|offer| (offer.kind, offer.hidden)).chain(
                    custom_ids
                        .iter()
                        .zip(&custom)
                        .map(|(id, profile)| (id.as_str(), profile.hidden)),
                ),
            )
            .map_err(|_| cli_control::Error)?;
            let visible = profiles::decode(&value).map_err(|_| cli_control::Error)?;
            let Some(selected) = selected else {
                return on_original_page(app, state, original, cancelled, move |_, _| Ok(value))
                    .await;
            };
            if !visible.contains(&selected.as_str()) {
                return Err(cli_control::Error);
            }
            let kind = super::session::kind(&selected).ok();
            let custom_id = if kind.is_none() {
                Some(uuid::Uuid::parse_str(&selected).map_err(|_| cli_control::Error)?)
            } else {
                None
            };
            let action = action.ok_or(cli_control::Error)?;
            let stop = cancelled.clone();
            let mut reservation = on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |window, current| {
                    let reservation = approvals
                        .reserve(current, stop, action)
                        .map_err(|_| cli_control::Error)?;
                    let mut notice =
                        json!({"version":1,"ticket":reservation.id.to_string(),"profile":selected});
                    if let Some(direction) = direction {
                        notice["direction"] = json!(direction);
                    }
                    if let Some(direction) = &cold_split_base {
                        notice = json!({"version":4,"stage":"base","ticket":reservation.id.to_string(),"profile":selected,"direction":direction});
                    } else if let Some(target) = cold_split_target {
                        notice["version"] = json!(4);
                        notice["stage"] = json!("split");
                        notice["target"] = json!(target);
                    }
                    event(&window, "pulseterm:cli-approval", notice)?;
                    eprintln!("workspace.cli_approval_requested");
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
            let action = tokio::select! { biased;
                _ = cancelled.cancelled() => return Err(cli_control::Error),
                result = tokio::time::timeout(approvals::WAIT, &mut reservation.answer) => result.map_err(|_| cli_control::Error)?
                    .map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error)?,
            };
            drop(reservation);
            let (mut inputs, settings_directory, system_dark) = on_original_page(
                app.clone(),
                state.clone(),
                original.clone(),
                cancelled.clone(),
                move |window, current| {
                    action
                        .take_args(&current, launch_action)
                        .map_err(|_| cli_control::Error)?;
                    Ok((
                        source(&window, kind.unwrap_or(pulse_pty::profiles::ShellKind::Sh))?,
                        window
                            .app_handle()
                            .path()
                            .app_config_dir()
                            .map_err(|_| cli_control::Error)?,
                        crate::product_workspace::settings::system_dark(&window),
                    ))
                },
            )
            .await?;
            inputs.preset = state
                .0
                .settings
                .terminal_startup(
                    state.0.registry.clone(),
                    original.clone(),
                    settings_directory,
                    system_dark,
                )
                .await
                .map_err(|_| cli_control::Error)?;
            let id = if let Some(id) = custom_id {
                state
                    .0
                    .profiles
                    .custom_start(
                        state.0.registry.clone(),
                        original.clone(),
                        state.0.sessions.clone(),
                        super::profiles::custom_service::CustomStart {
                            source: (inputs.directory, inputs.path, inputs.shell),
                            id,
                            startup: inputs.preset,
                            interrupt: Some(cancelled.clone()),
                        },
                    )
                    .await
                    .map_err(|_| cli_control::Error)?
            } else {
                let stop = cancelled.clone();
                state.0.profiles.start_interruptible(state.0.registry.clone(), original.clone(), state.0.sessions.clone(), inputs,
                    async move { tokio::select! { biased; _ = stop.cancelled() => {}, _ = tokio::time::sleep(Duration::from_secs(5)) => {} } })
                    .await.map_err(|_| cli_control::Error)?
            };
            // After root adoption a lost reply cannot undo a committed session. Never auto-retry.
            if !registration_only {
                return local_presentation::present(app, state, original, cancelled, ticket, id)
                    .await;
            }
            on_original_page(app, state, original, cancelled, move |_, _| {
                eprintln!("workspace.session_opened");
                eprintln!("workspace.cli_session_opened");
                Ok(json!({"session":id.get().to_string()}))
            })
            .await
        })
    }
}
impl Handler for Actions {
    fn operation_budget(&self, request: &Request) -> Duration {
        match request.method() {
            "workspace.bootstrap" => Duration::from_secs(90),
            "workspace.new-tab" | "workspace.new-window" | "workspace.split" | "workspace.ssh" => {
                Duration::from_secs(45)
            }
            _ => Duration::from_secs(5),
        }
    }
    fn call(&self, request: Request, cancelled: CancellationToken) -> Operation {
        if request.method() != "workspace.bootstrap" {
            let actions = self.clone();
            return Box::pin(async move {
                if cancelled.is_cancelled() {
                    return Err(cli_control::Error);
                }
                if request.method() == "workspace.windows" {
                    return windows::list(&actions.state.0.directory, &request);
                }
                let (state, request, page) = {
                    let contexts = actions
                        .app
                        .try_state::<super::window_state::Contexts<Workspace>>()
                        .ok_or(cli_control::Error)?;
                    windows::resolve(&actions.state.0.directory, &contexts, request)?
                };
                let approvals = state.0.cli_approvals.clone();
                Actions {
                    app: actions.app,
                    state,
                    approvals,
                }
                .call_scoped(request, CallOrigin::Routed(page), cancelled)
                .await
            });
        }
        let actions = self.clone();
        Box::pin(async move {
            // Authentication happened in the shared socket supervisor. This only
            // consumes metadata; the original action still goes through Capability.
            if cancelled.is_cancelled() || request.params().len() != 1 {
                return Err(cli_control::Error);
            }
            let id = request
                .params()
                .get("startup")
                .and_then(Value::as_str)
                .ok_or(cli_control::Error)?;
            let uuid = uuid::Uuid::parse_str(id).map_err(|_| cli_control::Error)?;
            if uuid.get_version_num() != 4 || uuid.to_string() != id {
                return Err(cli_control::Error);
            }
            let claim = actions
                .state
                .0
                .bootstrap
                .claim(id)
                .map_err(|_| cli_control::Error)?;
            if !claim.action().supports_cold_transport() {
                return Err(cli_control::Error);
            }
            let confirmation = matches!(
                claim.action().method(),
                "workspace.new-tab" | "workspace.split" | "workspace.ssh"
            );
            // Original-document confirmation and presentation share one 8s
            // metadata window. Neither grants action authority or holds native I/O.
            let original = tokio::select! { biased;
                _ = cancelled.cancelled() => return Err(cli_control::Error),
                result = tokio::time::timeout(Duration::from_secs(8), async {
                    let original = claim.original_scope(&actions.state.0.documents)
                        .await.map_err(|_| cli_control::Error)?;
                    if confirmation {
                        actions.state.0.presentation.wait(
                            &actions.state.0.documents, actions.state.0.owner,
                            &original, &cancelled,
                        ).await.map_err(|_| cli_control::Error)?;
                    }
                    Ok::<_, cli_control::Error>(original)
                }) => result.map_err(|_| cli_control::Error)??,
            };
            let wire = claim.action().encode().map_err(|_| cli_control::Error)?;
            let original_request =
                Request::parse(wire.as_bytes()).map_err(|_| cli_control::Error)?;
            // GUI approval retains its own 30s window, followed by the existing
            // native 5s startup limit. Do not wrap this chain in the readonly 5s.
            if confirmation {
                actions
                    .call_scoped(original_request, CallOrigin::Startup(original), cancelled)
                    .await
            } else {
                tokio::time::timeout(
                    Duration::from_secs(5),
                    actions.call_scoped(original_request, CallOrigin::Startup(original), cancelled),
                )
                .await
                .map_err(|_| cli_control::Error)?
            }
        })
    }
}

pub(super) fn start(app: &AppHandle, state: Workspace) -> cli_control::Result<Arc<Server>> {
    let directory = app
        .path()
        .runtime_dir()
        .map_err(|_| cli_control::Error)?
        .join("pulseterm-workspace-cli");
    let approvals = state.0.cli_approvals.clone();
    let handler = Arc::new(Actions {
        app: app.clone(),
        state,
        approvals,
    });
    Server::start(&directory, tauri::async_runtime::handle().inner(), handler).map(Arc::new)
}
