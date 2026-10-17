//! Shared display/login validation, not a parser for execution arguments.
pub(crate) fn valid(label: &str, username: &str) -> bool {
    !label.is_empty()
        && label.len() <= 256
        && label.trim() == label
        && !label.chars().any(|c| c.is_control() || invisible(c))
        && !username.is_empty()
        && username.len() <= 255
        && !username.starts_with('-')
        && !username
            .chars()
            .any(|c| c.is_control() || c.is_whitespace() || invisible(c))
}
fn invisible(c: char) -> bool {
    matches!(c,'\u{200b}'..='\u{200f}'|'\u{202a}'..='\u{202e}'|'\u{2060}'..='\u{206f}'|'\u{feff}')
}
