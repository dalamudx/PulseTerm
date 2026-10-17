//! Cold argv is metadata. Only the separate original-page confirmation captures an epoch.
use super::*;
pub(super) async fn confirm_intent(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    approvals: Arc<approvals::Approvals>,
    selected: (Uuid, Uuid, Option<&'static str>),
    authorization: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<pulse_app::vault_owner::SavedSessionGesture> {
    let authentication = selected.2;
    let vault = state.0.vault.clone();
    let stop = cancelled.clone();
    let mut reservation = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, current| {
            let intent = vault
                .begin_cold_saved_session_intent_authorized(
                    current.clone(),
                    selected.0,
                    selected.1,
                    authorization,
                )
                .map_err(|_| cli_control::Error)?;
            let reservation = approvals
                .reserve_intent(current, stop, intent, vault)
                .map_err(|_| cli_control::Error)?;
            event(
                &window,
                "pulseterm:cli-approval",
                json!({
                    "version":3, "ticket":reservation.id.to_string(),
                    "intent": if let Some(authentication) = authentication { json!({"hostId":selected.0.to_string(),"identityId":selected.1.to_string(),"authentication":authentication}) } else { json!({"hostId":selected.0.to_string(),"identityId":selected.1.to_string()}) }
                }),
            )?;
            eprintln!("workspace.cli_cold_ssh_intent_requested");
            Ok(reservation)
        },
    )
    .await?;
    let _notice = Notice {
        app,
        id: reservation.id,
        window_label: state
            .native_label()
            .map_err(|_| cli_control::Error)?
            .to_owned(),
    };
    tokio::select! { biased;
        _ = cancelled.cancelled() => Err(cli_control::Error),
        _ = tokio::time::sleep(approvals::WAIT) => Err(cli_control::Error),
        result = &mut reservation.answer => result.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error),
    }
}

pub(super) async fn confirm_alias(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    approvals: Arc<approvals::Approvals>,
    alias: String,
    authorization: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<pulse_app::vault_owner::SavedAliasGesture> {
    let vault = state.0.vault.clone();
    let stop = cancelled.clone();
    let mut reservation = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, current| {
            let intent = vault
                .begin_cold_saved_alias_intent_authorized(current.clone(), &alias, authorization)
                .map_err(|_| cli_control::Error)?;
            let reservation = approvals
                .reserve_alias(current, stop, intent, vault)
                .map_err(|_| cli_control::Error)?;
            event(
                &window,
                "pulseterm:cli-approval",
                json!({"version":3, "ticket":reservation.id.to_string(), "intent":{"alias":alias}}),
            )?;
            Ok(reservation)
        },
    )
    .await?;
    let _notice = Notice {
        app,
        id: reservation.id,
        window_label: state
            .native_label()
            .map_err(|_| cli_control::Error)?
            .to_owned(),
    };
    tokio::select! { biased;
        _ = cancelled.cancelled() => Err(cli_control::Error),
        _ = tokio::time::sleep(approvals::WAIT) => Err(cli_control::Error),
        result = &mut reservation.answer => result.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error),
    }
}
