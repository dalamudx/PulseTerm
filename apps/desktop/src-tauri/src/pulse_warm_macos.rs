//! macOS warm control only. Missing, stale or denied endpoints never launch a GUI or retry.
use pulse_app::cli_control::Client;
use serde_json::Value;
use std::path::Path;

type Result<T> = std::result::Result<T, &'static str>;

pub(super) async fn dispatch_connected(
    directory: &Path,
    method: &str,
    params: Value,
) -> Result<(Value, Client)> {
    let mut client = Client::connect(directory)
        .await
        .map_err(|_| "pulse.warm_denied")?;
    let value = client
        .invoke(method, params)
        .await
        .map_err(|_| "pulse.warm_denied")?;
    Ok((value, client))
}

pub(super) async fn dispatch(directory: &Path, method: &str, params: Value) -> Result<Value> {
    dispatch_connected(directory, method, params)
        .await
        .map(|(value, _)| value)
}
