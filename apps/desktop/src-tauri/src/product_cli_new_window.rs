//! Original-window approval creates one empty native window, never a terminal or a transfer.
use super::*;
pub(super) async fn run(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    action: PreparedWindowAction,
) -> cli_control::Result<Value> {
    let approvals = state.0.cli_approvals.clone();
    let stop = cancelled.clone();
    let page = original.clone();
    let mut reservation = on_original_page(
        app.clone(),
        state.clone(),
        original.clone(),
        cancelled.clone(),
        move |window, _| {
            let reservation = approvals
                .reserve(page, stop, action)
                .map_err(|_| cli_control::Error)?;
            event(
                &window,
                "pulseterm:cli-approval",
                json!({"version":5,"ticket":reservation.id.to_string()}),
            )?;
            eprintln!("workspace.cli_new_window_approval_requested");
            Ok(reservation)
        },
    )
    .await?;
    let _notice = Notice {
        app: app.clone(),
        id: reservation.id,
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
    let creating = state.clone();
    on_original_page(app, state, original, cancelled, move |window, current| {
        action
            .take_args(&current, "workspace.new-window")
            .map_err(|_| cli_control::Error)?;
        let label = super::super::new_window::create(window.app_handle(), &creating)
            .map_err(|_| cli_control::Error)?;
        Ok(json!({"window":label}))
    })
    .await
}
