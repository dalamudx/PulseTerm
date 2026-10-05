//! Pure scalar editing; no user-file IO, watcher, reset or atomic save authority.
use crate::settings::{SettingsError, SettingsPatch};
use toml_edit::{Document, DocumentMut, Item, Table, Value};
/// A caller cannot choose arbitrary paths, credential keys, tables or TOML fragments.
#[derive(Debug, Clone)]
pub enum TerminalSettingEdit {
    /// Set the supported numeric size.
    FontSize(u16),
    /// Set a bounded CSS-safe Unicode font name.
    FontFamily(String),
    /// False remains an explicit override.
    CursorBlink(bool),
}
/// Set only one known scalar, retaining key/value decor and all other entries.
/// Every original field and the new field validate; malformed input is not reset.
/// Removing overrides needs a separate comment-preserving design and is not supported.
pub fn set_terminal_setting(
    text: &str,
    edit: TerminalSettingEdit,
) -> Result<String, SettingsError> {
    let mut expected = SettingsPatch::parse(text)?;
    let (key, replacement) = match edit {
        TerminalSettingEdit::FontSize(value) => {
            expected.terminal.font_size = Some(value);
            ("font_size", Value::from(i64::from(value)))
        }
        TerminalSettingEdit::FontFamily(value) => {
            expected.terminal.font_family = Some(value.clone());
            ("font_family", Value::from(value))
        }
        TerminalSettingEdit::CursorBlink(value) => {
            expected.terminal.cursor_blink = Some(value);
            ("cursor_blink", Value::from(value))
        }
    };
    expected.validate()?;
    let raw = Document::parse(text).map_err(|_| SettingsError::Document)?;
    let parent = raw.as_table().get("terminal");
    let existing = parent
        .and_then(Item::as_table_like)
        .and_then(|table| table.get(key));
    let result = if let Some(existing) = existing {
        // Mutable serialization can normalize every repeated dotted parent key.
        // Replace only the immutable parser's exact value-token byte range.
        let span = existing
            .as_value()
            .and_then(Value::span)
            .ok_or(SettingsError::Document)?;
        let prefix = text.get(..span.start).ok_or(SettingsError::Document)?;
        let suffix = text.get(span.end..).ok_or(SettingsError::Document)?;
        format!("{prefix}{replacement}{suffix}")
    } else if parent
        .and_then(Item::as_table)
        .is_some_and(Table::is_dotted)
    {
        let separator = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        format!("{text}{separator}terminal.{key} = {replacement}\n")
    } else {
        let mut document: DocumentMut = text.parse().map_err(|_| SettingsError::Document)?;
        if !document.contains_key("terminal") {
            document.insert("terminal", Item::Table(Table::new()));
        }
        let table = document
            .get_mut("terminal")
            .and_then(Item::as_table_like_mut)
            .ok_or(SettingsError::Document)?;
        table.insert(key, Item::Value(replacement));
        document.to_string()
    };
    if SettingsPatch::parse(&result)? != expected {
        return Err(SettingsError::Document);
    }
    Ok(result)
}
#[cfg(test)]
#[path = "settings_edit_tests.rs"]
mod tests;
