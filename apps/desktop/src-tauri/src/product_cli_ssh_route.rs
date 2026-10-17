//! CLI route handoff consumes the original saved snapshot and page capture. No second GUI begin.
use super::super::present_registered_session;
use super::{Workspace, event, on_original_page};
use pulse_app::{cli_control, registry::PageLaunchScope, vault_owner::SavedSessionGesture};
use pulse_store::{StateStore, ssh_route_source::SshRouteSource};
use std::sync::Arc;
use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

pub(super) async fn run(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    store: StateStore,
    captured: SavedSessionGesture,
    source: SshRouteSource,
) -> cli_control::Result<serde_json::Value> {
    let palette = super::super::super::ssh_connection::startup_palette(
        &app,
        &state,
        original.clone(),
        source.hops().last().map(|hop| hop.entrance_id()),
    )
    .await
    .map_err(|_| cli_control::Error)?;
    let bank = state.0.ssh_sessions.native_routes();
    let issued = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |_, page| {
            bank.begin_from_original(page, captured)
                .map_err(|_| cli_control::Error)
        },
    )
    .await?;
    let id = issued.id;
    let mut cancel = issued.cancellation_guard();
    let consumed = issued.completion_notification();
    let (sent, result) = tokio::sync::oneshot::channel();
    let registry = state.0.registry.clone();
    let root = state.0.sessions.clone();
    let owner = state.0.vault.clone();
    let page = original.clone();
    let broker = issued.broker();
    issued.launch(
        tauri::async_runtime::handle().inner(),
        move |capture, confirm| async move {
            let outcome = async {
                let route = capture
                    .select_route(source)
                    .map_err(|_| "workspace.denied")?;
                crate::ssh_session_adapter::route::run(
                    crate::ssh_session_adapter::route::Context {
                        store,
                        registry,
                        page,
                        owner,
                        root,
                        size: pulse_core::TermSize::new(80, 24).map_err(|_| "workspace.denied")?,
                        history_rows: 10000,
                        palette,
                        broker: Some(broker),
                    },
                    route,
                    confirm,
                )
                .await
            }
            .await;
            let _ = sent.send(outcome);
            outcome
        },
    );
    let documents = state.0.documents.clone();
    let window_owner = state.0.owner;
    on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, current| {
            let (nonce, exact) = documents
                .current_native_document(window_owner)
                .map_err(|_| cli_control::Error)?;
            if !Arc::ptr_eq(&current, &exact) {
                return Err(cli_control::Error);
            }
            event(
                &window,
                "pulseterm:cli-ssh-route",
                serde_json::json!({
                    "version":1, "nonce":nonce.to_string(), "token":id.to_string()
                }),
            )?;
            eprintln!("workspace.cli_ssh_route_requested");
            Ok(())
        },
    )
    .await?;
    let session = tokio::select! { biased;
        _=cancelled.cancelled()=>return Err(cli_control::Error),
        outcome=tokio::time::timeout(std::time::Duration::from_secs(300), result)=>
            outcome.map_err(|_|cli_control::Error)?.map_err(|_|cli_control::Error)?.map_err(|_|cli_control::Error)?,
    };
    // A successful Root handoff is immutable. Keep the request alive until the original page
    // consumes Bank Done, so socket EOF cannot revoke its final wait before the reply is committed.
    tokio::select! { biased;
        _=cancelled.cancelled()=>return Err(cli_control::Error),
        outcome=tokio::time::timeout(std::time::Duration::from_secs(5), consumed.notified())=>
            outcome.map_err(|_|cli_control::Error)?,
    }
    present_registered_session(app, state, original, cancelled, session).await?;
    eprintln!("workspace.cli_ssh_route_opened");
    cancel.disarm();
    Ok(serde_json::json!({"session":session.get().to_string()}))
}
