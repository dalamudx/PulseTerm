//! Bounded command sequence; separators are argv tokens, never shell source.
use serde_json::Value;
use std::ffi::{OsStr, OsString};

pub(super) const MAX_STEPS: usize = 8;
pub(super) type Step = (&'static str, Value);
pub(super) fn parse(args: impl IntoIterator<Item = OsString>) -> Option<Vec<Step>> {
    let args: Vec<_> = args.into_iter().take(65).collect();
    if args.is_empty() || args.len() > 64 {
        return None;
    }
    let mut steps = Vec::new();
    let mut cursor = 0;
    while cursor < args.len() {
        if steps.len() == MAX_STEPS {
            return None;
        }
        let prefix = usize::from(args[cursor] == OsStr::new("-w")) * 2;
        let action = args.get(cursor + prefix)?.to_str()?;
        let width = match action {
            "." if cursor == 0 && args.len() == prefix + 1 => 1,
            "focus" | "profiles" | "windows" | "new-window" => 1,
            "new-tab" | "focus-tab" => 2,
            "open" if args.get(cursor + prefix + 1)? == OsStr::new("--host") => 3,
            "open" => 2,
            "split" | "split-pane" => {
                if args
                    .get(cursor + prefix + 2)
                    .is_none_or(|arg| matches!(arg.to_str(), Some("." | ";")))
                {
                    2
                } else {
                    3
                }
            }
            "ssh"
                if args.get(cursor + prefix + 4).is_some_and(|arg| {
                    matches!(
                        arg.to_str(),
                        Some(
                            "--interactive"
                                | "--key-interactive"
                                | "--agent-interactive"
                                | "--password-interactive"
                        )
                    )
                }) =>
            {
                5
            }
            "ssh" => 4,
            _ => return None,
        };
        let end = cursor.checked_add(prefix + width)?;
        let step = super::request(args.get(cursor..end)?.iter().cloned())?;
        steps.push(step);
        cursor = end;
        if cursor == args.len() {
            break;
        }
        if !matches!(args[cursor].to_str(), Some("." | ";")) {
            return None;
        }
        cursor += 1;
        if cursor == args.len() {
            return None;
        }
    }
    Some(steps)
}

#[cfg(test)]
#[path = "pulse_chain_tests.rs"]
mod tests;
