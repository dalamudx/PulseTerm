use super::*;

// One explicit approval per native start. Registered sessions remain root-owned on failure.
pub(super) async fn run(
    actions: Actions,
    request: Request,
    original: Arc<PageLaunchScope>,
    cancelled: CancellationToken,
) -> cli_control::Result<Value> {
    // Freeze the saved identity once, so both cold-start approvals use the same Profile.
    let request = if request.params().contains_key("profile") {
        request
    } else {
        let profile = saved_default(
            actions.app.clone(),
            actions.state.clone(),
            original.clone(),
            cancelled.clone(),
        )
        .await?;
        let mut params = request.params().clone();
        params.insert("profile".into(), json!(profile));
        let wire = serde_json::to_vec(&json!({"jsonrpc":"2.0", "id":request.id(), "method":request.method(), "params":params})).map_err(|_| cli_control::Error)?;
        Request::parse(&wire).map_err(|_| cli_control::Error)?
    };
    let profile = request
        .params()
        .get("profile")
        .and_then(Value::as_str)
        .ok_or(cli_control::Error)?;
    let direction = request
        .params()
        .get("direction")
        .and_then(Value::as_str)
        .ok_or(cli_control::Error)?
        .to_owned();
    if !matches!(direction.as_str(), "row" | "column") {
        return Err(cli_control::Error);
    }
    let base_wire = serde_json::to_string(&json!({
        "jsonrpc":"2.0", "id":uuid::Uuid::new_v4().to_string(),
        "method":"workspace.new-tab", "params":{"profile":profile},
    }))
    .map_err(|_| cli_control::Error)?;
    let base = actions
        .call_scoped(
            Request::parse(base_wire.as_bytes()).map_err(|_| cli_control::Error)?,
            CallOrigin::SplitBase {
                page: original.clone(),
                direction,
            },
            cancelled.clone(),
        )
        .await?;
    let id = base
        .get("session")
        .and_then(Value::as_str)
        .ok_or(cli_control::Error)?;
    let session = actions
        .state
        .0
        .sessions
        .lock()
        .map_err(|_| cli_control::Error)?
        .ids()
        .into_iter()
        .find(|session| session.get().to_string() == id)
        .ok_or(cli_control::Error)?;
    // Same original-page typed Root selection bridge used by SSH; no focus capability transfer.
    present_registered_session(
        actions.app.clone(),
        actions.state.clone(),
        original.clone(),
        cancelled.clone(),
        session,
    )
    .await?;
    actions
        .call_scoped(
            request,
            CallOrigin::SplitFollowup {
                page: original,
                target: session,
            },
            cancelled,
        )
        .await
}
