#![allow(clippy::unwrap_used)]
use super::*;
fn parse_text(args: &[&str]) -> Option<Vec<Step>> {
    parse(args.iter().map(OsString::from))
}
#[test]
fn chain_separators_only_follow_complete_actions_and_preserve_exact_aliases() {
    let steps = parse_text(&[
        "-w",
        "workspace",
        "open",
        "--host",
        ".",
        ";",
        "profiles",
        ".",
        "focus",
    ])
    .unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(
        steps[0],
        (
            "workspace.ssh",
            serde_json::json!({"alias":".","window":"workspace"})
        )
    );
    assert_eq!(steps[1].0, "workspace.profiles");
    assert_eq!(steps[2].0, "workspace.focus");
    let steps = parse_text(&["open", "--host", ";", ".", "focus"]).unwrap();
    assert_eq!(steps[0].1["alias"], ";");
    assert!(parse_text(&["focus;profiles"]).is_none());
}
#[test]
fn every_step_is_checked_before_any_action_and_empty_or_ninth_steps_fail() {
    for args in [
        vec![],
        vec![".", "focus"],
        vec!["focus", ";"],
        vec!["focus", ".", ";", "focus"],
        vec!["focus", ".", "new-tab", "/bin/sh"],
        vec!["profiles", ";", "ssh", "bad", "--identity", "bad"],
        vec!["focus", ".", "-w", "missing", "profiles"],
        vec!["focus", ".", "open", "--host", "bad\nname"],
    ] {
        assert!(parse_text(&args).is_none(), "{args:?}");
    }
    let eight = std::iter::repeat_n("focus", 8)
        .collect::<Vec<_>>()
        .join(" . ");
    assert_eq!(
        parse_text(&eight.split(' ').collect::<Vec<_>>())
            .unwrap()
            .len(),
        MAX_STEPS
    );
    let nine = format!("{eight} . focus");
    assert!(parse_text(&nine.split(' ').collect::<Vec<_>>()).is_none());
    assert!(parse(std::iter::repeat_n(OsString::from("focus"), 65)).is_none());
}
#[cfg(unix)]
#[test]
fn non_unicode_later_step_rejects_whole_chain() {
    use std::os::unix::ffi::OsStringExt;
    assert!(
        parse([
            OsString::from("focus"),
            OsString::from("."),
            OsString::from("open"),
            OsString::from_vec(vec![0xff])
        ])
        .is_none()
    );
}

#[test]
fn interactive_ssh_chain_uses_one_explicit_extra_token() {
    let host = "01956c1c-1a90-7000-8000-000000000011";
    let id = "01956c1c-1a90-7000-8000-000000000012";
    let steps = parse_text(&[
        "-w",
        "workspace",
        "ssh",
        host,
        "--identity",
        id,
        "--interactive",
        ".",
        "profiles",
    ])
    .unwrap();
    assert_eq!(steps.len(), 2);
    assert_eq!(steps[0].1["authentication"], "interactive");
    assert_eq!(steps[1].0, "workspace.profiles");
    assert!(parse_text(&["ssh", host, "--identity", id, "--password", ".", "profiles"]).is_none());
}

#[test]
fn key_interactive_chain_keeps_exact_window_method_and_step_boundaries() {
    let host = "01956c1c-1a90-7000-8000-000000000011";
    let identity = "01956c1c-1a90-7000-8000-000000000012";
    let steps = parse_text(&[
        "-w",
        "workspace",
        "ssh",
        host,
        "--identity",
        identity,
        "--key-interactive",
        ";",
        "profiles",
        ".",
        "focus",
    ])
    .unwrap();
    assert_eq!(steps.len(), 3);
    assert_eq!(
        steps[0],
        (
            "workspace.ssh",
            serde_json::json!({
                "host": host, "identity": identity, "authentication": "key-interactive", "window": "workspace"
            })
        )
    );
    assert_eq!(steps[1].0, "workspace.profiles");
    assert_eq!(steps[2].0, "workspace.focus");
    for tail in [
        vec!["--key-interactive", "--interactive"],
        vec!["--interactive", "--key-interactive"],
        vec!["--key-interactive", "--key-interactive"],
        vec!["--key-interactive=false"],
    ] {
        let mut args = vec!["ssh", host, "--identity", identity];
        args.extend(tail);
        assert!(parse_text(&args).is_none());
    }
}
