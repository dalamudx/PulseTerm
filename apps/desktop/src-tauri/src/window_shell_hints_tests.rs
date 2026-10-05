use super::*;
use tauri::webview::PageLoadEvent;
#[test]
fn page_hint_receipt_is_single_use_and_navigation_does_not_reuse_it() -> Result<()> {
    let p = reload::PageTracker::new();
    p.record(PageLoadEvent::Started);
    p.record(PageLoadEvent::Finished);
    let old = p.injection()?.0;
    p.confirm_nonce(&old)?;
    assert!(p.mark_shell_hints(&old).is_err());
    p.mark_captured(&old)?;
    assert!(p.require_shell_hints(&old).is_err());
    p.mark_shell_hints(&old)?;
    p.require_shell_hints(&old)?;
    assert!(p.mark_shell_hints(&old).is_err());
    assert!(p.authorize_shell_hints(&old).is_err());
    p.record(PageLoadEvent::Started);
    assert!(p.require_shell_hints(&old).is_err());
    p.record(PageLoadEvent::Finished);
    let fresh = p.injection()?.0;
    assert!(p.confirm_nonce(&old).is_err());
    p.confirm_nonce(&fresh)?;
    assert!(p.require_shell_hints(&fresh).is_err());
    p.mark_captured(&fresh)?;
    p.mark_shell_hints(&fresh)?;
    p.require_shell_hints(&fresh)?;
    Ok(())
}
#[test]
fn hints_are_typed_advisories_and_exit_status_is_nullable() -> serde_json::Result<()> {
    assert_eq!(
        serde_json::to_value(ShellHint::from(
            pulse_term::shell_markers::ShellMarker::PromptEnd
        ))?,
        serde_json::json!({"kind":"promptEnd"})
    );
    assert_eq!(
        serde_json::to_value(ShellHint::CommandEnd { exit_status: None })?,
        serde_json::json!({"kind":"commandEnd","exitStatus":null})
    );
    assert_eq!(
        serde_json::to_value(ShellHint::CommandEnd {
            exit_status: Some(7)
        })?,
        serde_json::json!({"kind":"commandEnd","exitStatus":7})
    );
    Ok(())
}
