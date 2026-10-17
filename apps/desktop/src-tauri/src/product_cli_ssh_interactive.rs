//! Original-window, data-only challenge delivery for explicitly approved CLI MFA.
use crate::ssh_session_adapter::{interactive::Broker, requests::SessionRequests};
use pulse_app::{
    registry::PageLaunchScope,
    vault_owner::{InteractiveSessionGesture, PageVaultOwner, VaultOwnerError},
};
use std::sync::{Arc, OnceLock};
use tauri::{Emitter, WebviewWindow};
use uuid::Uuid;

pub(super) enum Gesture {
    Interactive(InteractiveSessionGesture),
    PrivateKey(pulse_app::vault_owner::PrivateKeyInteractiveSessionGesture),
}
pub(super) fn begin(
    window: WebviewWindow,
    page: Arc<PageLaunchScope>,
    requests: &Arc<SessionRequests>,
    vault: &PageVaultOwner,
    selected: (Uuid, Uuid),
    gesture: Gesture,
    approval: Uuid,
) -> Result<Uuid, &'static str> {
    let token = Arc::new(OnceLock::<Uuid>::new());
    let captured = token.clone();
    let broker = Broker::new(move |prompt| {
        let token = captured.get().ok_or(VaultOwnerError::Denied)?;
        // Remote text travels as serialized event data; it never enters eval/script source.
        window.emit_to(window.label(), "pulseterm:cli-interactive", serde_json::json!({
            "version":1, "ticket":approval.to_string(), "token":token.to_string(), "prompt":prompt
        })).map_err(|_| VaultOwnerError::Denied)
    });
    let id = match gesture {
        Gesture::Interactive(gesture) => requests.begin_interactive_gesture(
            page.clone(),
            selected.0,
            selected.1,
            vault,
            gesture,
            broker,
        ),
        Gesture::PrivateKey(gesture) => requests.begin_key_interactive_gesture(
            page.clone(),
            selected.0,
            selected.1,
            vault,
            gesture,
            broker,
        ),
    }?;
    if token.set(id).is_err() {
        let _ = requests.cancel(&page, id);
        return Err("workspace.cli_forbidden");
    }
    Ok(id)
}
