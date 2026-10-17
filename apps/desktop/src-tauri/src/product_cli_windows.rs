//! Native window metadata routing. Selection never substitutes page or effect admission.
use super::*;
use pulse_app::window_directory::{WindowDirectory, WindowLabel};
#[path = "cli_windows.rs"]
mod projection;

pub(super) fn list(directory: &WindowDirectory, request: &Request) -> cli_control::Result<Value> {
    let catalogue =
        capabilities::catalogue(cfg!(feature = "workspace-ssh")).map_err(|_| cli_control::Error)?;
    catalogue
        .validate_args(
            request.method(),
            Surface::Cli,
            Value::Object(request.params().clone()),
        )
        .map_err(|_| cli_control::Error)?;
    if request.method() != "workspace.windows"
        || catalogue
            .policy(request.method(), Surface::Cli)
            .map_err(|_| cli_control::Error)?
            != (pulse_cap::Scope::Global, pulse_cap::Risk::Safe)
    {
        return Err(cli_control::Error);
    }
    let labels = directory.list().map_err(|_| cli_control::Error)?;
    let value = json!({"version":1,"windows":labels.iter().map(|label| label.as_str()).collect::<Vec<_>>()});
    projection::decode(&value).ok_or(cli_control::Error)?;
    Ok(value)
}
pub(super) fn resolve(
    directory: &WindowDirectory,
    contexts: &super::super::window_state::Contexts<Workspace>,
    request: Request,
) -> cli_control::Result<(Workspace, Request, Arc<PageLaunchScope>)> {
    let mut params = request.params().clone();
    let label = params
        .remove("window")
        .map(|value| {
            let text = value.as_str().ok_or(cli_control::Error)?;
            WindowLabel::parse(text).map_err(|_| cli_control::Error)
        })
        .transpose()?;
    let route = directory
        .select(label.as_ref())
        .map_err(|_| cli_control::Error)?;
    let state = contexts
        .for_native_label(route.label())
        .map_err(|_| cli_control::Error)?;
    if state.native_label().ok() != Some(route.label().as_str()) {
        return Err(cli_control::Error);
    }
    let page = directory.current(&route).map_err(|_| cli_control::Error)?;
    let exact = state
        .0
        .documents
        .current_native(state.0.owner)
        .map_err(|_| cli_control::Error)?;
    if !Arc::ptr_eq(&page, &exact) {
        return Err(cli_control::Error);
    }
    let wire = serde_json::to_vec(
        &json!({"jsonrpc":"2.0","id":request.id(),"method":request.method(),"params":params}),
    )
    .map_err(|_| cli_control::Error)?;
    let request = Request::parse(&wire).map_err(|_| cli_control::Error)?;
    Ok((state, request, page))
}
#[cfg(test)]
#[path = "product_cli_windows_tests.rs"]
mod tests;
