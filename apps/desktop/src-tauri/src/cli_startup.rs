//! Bounded, metadata-only startup parcel. Parsing is not admission or execution.
//!
//! A correlation id is not authority. The native single-instance adapter must still
//! bind the original window/page, validate Capability and frozen sources, and obtain
//! any required GUI confirmation. Never put tokens, passwords, paths or cwd here.
use pulse_ipc::cli::Request;
use serde_json::{Map, Value, json};
use std::ffi::{OsStr, OsString};

/// Smaller than the transport limit: one closed startup action, not a batch.
pub const MAX_STARTUP_BYTES: usize = 4096;
/// Content-free rejection; never format argv or request data.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("workspace.startup_denied")]
pub struct Error;
/// Immutable metadata only. This does not carry a page scope, key or handler.
pub struct StartupAction {
    request: Request,
}
impl StartupAction {
    /// Decode one JSON-RPC parcel, including nested duplicate-key rejection.
    pub fn parse(text: &str) -> Result<Self, Error> {
        if text.is_empty() || text.len() > MAX_STARTUP_BYTES {
            return Err(Error);
        }
        let mut request = Request::parse(text.as_bytes()).map_err(|_| Error)?;
        // Cold startup may target only the newly created primary; never recreate a named window.
        if let Some(label) = request.params().get("window") {
            if label.as_str() != Some("workspace") {
                return Err(Error);
            }
            let mut params = request.params().clone();
            params.remove("window");
            let wire = serde_json::to_vec(&json!({"jsonrpc":"2.0","id":request.id(),"method":request.method(),"params":params})).map_err(|_| Error)?;
            request = Request::parse(&wire).map_err(|_| Error)?;
        }
        canonical_uuid(request.id(), 4)?;
        let params = request.params();
        let fields: &[&str] = match request.method() {
            "workspace.focus" | "workspace.profiles" => &[],
            "workspace.new-tab" => &["profile"],
            "workspace.split" => &["profile", "direction"],
            "workspace.ssh" if params.contains_key("alias") => &["alias"],
            "workspace.ssh" if params.contains_key("authentication") => {
                &["host", "identity", "authentication"]
            }
            "workspace.ssh" => &["host", "identity"],
            _ => return Err(Error),
        };
        if params.len() != fields.len() || fields.iter().any(|key| !params.contains_key(*key)) {
            return Err(Error);
        }
        match request.method() {
            "workspace.new-tab" | "workspace.split" => {
                let profile = params.get("profile").and_then(Value::as_str).ok_or(Error)?;
                if pulse_store::profile_default::DefaultProfileSelection::parse(profile).is_none() {
                    return Err(Error);
                }
                if request.method() == "workspace.split"
                    && !matches!(
                        params.get("direction").and_then(Value::as_str),
                        Some("row" | "column")
                    )
                {
                    return Err(Error);
                }
            }
            "workspace.ssh" if params.contains_key("alias") => {
                let alias = params.get("alias").and_then(Value::as_str).ok_or(Error)?;
                pulse_core::ssh_endpoint::SshEndpoint::new(alias, "alias.invalid", 22, "metadata")
                    .map_err(|_| Error)?;
            }
            "workspace.ssh" => {
                if params.get("authentication").is_some_and(|v| {
                    !matches!(
                        v.as_str(),
                        Some("interactive" | "key-interactive" | "agent-interactive")
                    )
                }) {
                    return Err(Error);
                }
                for key in ["host", "identity"] {
                    canonical_uuid(params.get(key).and_then(Value::as_str).ok_or(Error)?, 7)?;
                }
            }
            _ => {}
        }
        Ok(Self { request })
    }
    /// Build an inert parcel for an already parsed CLI action.
    pub fn new(method: &str, params: &Value) -> Result<Self, Error> {
        let text = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": uuid::Uuid::new_v4().to_string(),
            "method": method, "params": params
        }))
        .map_err(|_| Error)?;
        Self::parse(&text)
    }
    /// Canonical wire metadata for the native startup parameter, never secrets.
    pub fn encode(&self) -> Result<String, Error> {
        let text = serde_json::to_string(&json!({
            "jsonrpc": "2.0", "id": self.request.id(),
            "method": self.request.method(), "params": self.request.params()
        }))
        .map_err(|_| Error)?;
        if text.len() > MAX_STARTUP_BYTES {
            return Err(Error);
        }
        Ok(text)
    }
    /// Correlation only, not a capability or cancellation authorization.
    pub fn request_id(&self) -> &str {
        self.request.id()
    }
    /// A closed action name; still needs independent Capability admission.
    pub fn method(&self) -> &str {
        self.request.method()
    }
    /// Closed cold-transport mask only; never Capability or execution authority.
    /// Page/source checks and the actual action's native deadline still apply.
    pub fn supports_cold_readonly_transport(&self) -> bool {
        matches!(self.method(), "workspace.focus" | "workspace.profiles")
    }
    /// Closed cold-start transport support; never admission or execution authority.
    /// New-tab and SSH require original-page presentation and separate explicit GUI approval.
    /// Secondary GUI forwarding remains limited to `supports_cold_readonly_transport`.
    pub fn supports_cold_transport(&self) -> bool {
        self.supports_cold_readonly_transport()
            || matches!(
                self.method(),
                "workspace.new-tab" | "workspace.split" | "workspace.ssh"
            )
    }
    /// Read-only validated metadata; saved UUIDs are not credential permissions.
    pub fn params(&self) -> &Map<String, Value> {
        self.request.params()
    }
}
fn canonical_uuid(text: &str, version: usize) -> Result<(), Error> {
    let id = uuid::Uuid::parse_str(text).map_err(|_| Error)?;
    if id.get_version_num() != version
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != text
    {
        return Err(Error);
    }
    Ok(())
}
/// Parse arguments after argv[0]. No action is the normal GUI invocation.
/// Reject unknown flags, multiple actions, non-Unicode data and extra arguments.
pub fn from_args(args: impl IntoIterator<Item = OsString>) -> Result<Option<StartupAction>, Error> {
    let mut args = args.into_iter();
    let Some(flag) = args.next() else {
        return Ok(None);
    };
    if flag != OsStr::new("--startup-actions") {
        return Err(Error);
    }
    let value = args.next().ok_or(Error)?;
    if args.next().is_some() {
        return Err(Error);
    }
    StartupAction::parse(value.to_str().ok_or(Error)?).map(Some)
}
#[cfg(test)]
#[path = "cli_startup_tests.rs"]
mod tests;
