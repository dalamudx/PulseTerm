//! Fixed-page advisory metadata. No command text, coordinates or session selection.
use super::*;
use serde::Serialize;
#[cfg(test)]
#[path = "window_shell_hints_tests.rs"]
mod tests;
#[derive(Debug, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub(crate) enum ShellHint {
    PromptStart,
    PromptEnd,
    CommandStart,
    CommandEnd {
        #[serde(rename = "exitStatus")]
        exit_status: Option<u8>,
    },
}
#[derive(Debug, Serialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ShellHints {
    sampled_revision: String,
    sampled_source_bytes: String,
    sampled_display_bytes: String,
    dropped: String,
    markers: Vec<ShellHint>,
    login_fixture: bool,
    initial_page: bool,
}
impl From<pulse_term::shell_markers::ShellMarker> for ShellHint {
    fn from(marker: pulse_term::shell_markers::ShellMarker) -> Self {
        use pulse_term::shell_markers::ShellMarker;
        match marker {
            ShellMarker::PromptStart => Self::PromptStart,
            ShellMarker::PromptEnd => Self::PromptEnd,
            ShellMarker::CommandStart => Self::CommandStart,
            ShellMarker::CommandEnd { exit_status } => Self::CommandEnd { exit_status },
        }
    }
}
#[tauri::command]
#[specta::specta]
pub(crate) async fn probe_peer_shell_hints(
    window: WebviewWindow,
    state: State<'_, WindowSessions>,
    nonce: String,
    page_nonce: String,
    generation: String,
) -> Result<ShellHints> {
    let (owner, id, pages) = peer_view::owned(&state, &window, &nonce, &page_nonce)?;
    pages.authorize_shell_hints(&page_nonce)?;
    let generation = parse_u64(&generation)?;
    if transfer_view::live_generation(&state, owner, id)? != generation {
        return Err("session.peer_shell_hints_stale".into());
    }
    let sampled = state
        .0
        .registry
        .take_shell_markers(owner, id, generation)
        .await
        .map_err(error)?;
    peer_view::owned(&state, &window, &nonce, &page_nonce)?;
    let initial_page = pages.initial_page(&page_nonce)?;
    let markers: Vec<_> = sampled
        .hints
        .markers
        .into_iter()
        .map(ShellHint::from)
        .collect();
    let expected = if cfg!(feature = "login-probe") && initial_page {
        vec![
            ShellHint::PromptStart,
            ShellHint::PromptEnd,
            ShellHint::CommandStart,
        ]
    } else {
        Vec::new()
    };
    if sampled.hints.dropped != 0 || markers != expected {
        return Err("session.peer_shell_hints_mismatch".into());
    }
    state
        .0
        .registry
        .check_binding(owner, id, generation)
        .map_err(error)?;
    pages.mark_shell_hints(&page_nonce)?;
    Ok(ShellHints {
        sampled_revision: sampled.sampled_at.revision.to_string(),
        sampled_source_bytes: sampled.sampled_at.parsed_bytes.to_string(),
        sampled_display_bytes: sampled.sampled_at.utf8_bytes.to_string(),
        dropped: sampled.hints.dropped.to_string(),
        markers,
        login_fixture: cfg!(feature = "login-probe"),
        initial_page,
    })
}
