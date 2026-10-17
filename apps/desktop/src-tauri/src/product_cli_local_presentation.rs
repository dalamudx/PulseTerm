use super::*;

// Registration is not presentation. The receipt is scoped to this exact native
// session and original page; layout/input still use their ordinary authorization.
pub(super) async fn present(
    app: AppHandle,
    state: Workspace,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
    ticket: uuid::Uuid,
    session: pulse_app::registry::SessionId,
) -> cli_control::Result<Value> {
    let root = state.clone();
    let stop = cancelled.clone();
    let mut reservation = on_original_page(app.clone(), state.clone(), original.clone(), cancelled.clone(), move |window, current| {
        let target = owned_selection(&root, &session.get().to_string())?;
        let (nonce, exact) = root.0.documents.current_native_document(root.0.owner).map_err(|_| cli_control::Error)?;
        if !Arc::ptr_eq(&current, &exact) { return Err(cli_control::Error); }
        let reservation = root.0.selections.begin(current, target, stop).map_err(|_| cli_control::Error)?;
        eprintln!("workspace.session_opened");
        eprintln!("workspace.cli_session_opened");
        event(&window, "pulseterm:cli-session", json!({"version":5,"ticket":ticket.to_string(),"session":target.to_string(),"nonce":nonce.to_string(),"presentation":reservation.id.to_string()}))?;
        Ok(reservation)
    }).await?;
    let _notice = Notice {
        app: app.clone(),
        id: reservation.id,
        window_label: state
            .native_label()
            .map_err(|_| cli_control::Error)?
            .to_owned(),
    };
    tokio::select! { biased;
        _ = cancelled.cancelled() => return Err(cli_control::Error),
        result = tokio::time::timeout(Duration::from_secs(5), &mut reservation.answer) => result.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error)?.map_err(|_| cli_control::Error)?,
    }
    on_original_page(app, state.clone(), original, cancelled, move |window, _| {
        owned_selection(&state, &session.get().to_string())?;
        window.set_focus().map_err(|_| cli_control::Error)?;
        eprintln!("workspace.cli_local_presentation_confirmed");
        Ok(json!({"session":session.get().to_string()}))
    })
    .await
}
