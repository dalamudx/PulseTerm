//! Pure scalar editing; no user-file IO, watcher, reset or atomic save authority.
use crate::settings::{SettingsError, SettingsPatch};
use toml_edit::{Document, DocumentMut, Item, Table, Value};
/// A caller cannot choose arbitrary paths, credential keys, tables or TOML fragments.
#[derive(Debug, Clone)]
pub enum TerminalSettingEdit {
    /// Bound retained primary history for a new session.
    HistoryLimit(u32),
    /// Select a fixed terminal base for new local sessions.
    ColorPreset(crate::settings::TerminalPreset),
    /// Save the user-level system-theme policy atomically.
    Theme(crate::settings::TerminalTheme),
    /// Set the supported numeric size.
    FontSize(u16),
    /// Set the bounded cell-height percentage.
    LineHeightPercent(u16),
    /// Set additional cell width in whole CSS pixels.
    LetterSpacing(u16),
    /// Set a bounded CSS-safe Unicode font name.
    FontFamily(String),
    /// Set one bounded fallback family; never a raw CSS list or URL.
    FontFallback(String),
    /// Default-off shaping for the built-in font’s programming operators.
    FontLigatures(bool),
    /// False remains an explicit override.
    CursorBlink(bool),
    /// Set del or bs for plain Backspace, never arbitrary bytes.
    BackspaceSend(String),
    /// Plain Delete encoding; no arbitrary bytes.
    DeleteSend(String),
    /// Advertise a fixed terminal type at startup.
    TermType(crate::settings::TerminalType),
    /// Control only the viewport response to authorized terminal input.
    ScrollOnInput(bool),
    /// Force the live viewport after parsed output; default false.
    ScrollOnOutput(bool),
    /// Set the default-off macOS Option-as-Meta preference.
    MacOptionAsMeta(bool),
    /// Set the explicit, default-off user selection copy preference.
    CopyOnSelect(bool),
    /// Set explicit default-off right-click paste.
    RightClickPaste(bool),
    /// Set the session close confirmation preference.
    ConfirmClose(bool),
    /// Set a fixed user-level interface mode, without changing terminal colors.
    UiAppearance(crate::settings::UiAppearance),
    /// Select one fixed UI accent family; user-level only.
    UiAccent(crate::settings::UiAccent),
    /// Change the user-level interface language.
    UiLanguage(crate::settings::UiLanguage),
}
/// Set only one known scalar, retaining key/value decor and all other entries.
/// Every original field and the new field validate; malformed input is not reset.
/// See `settings_remove` for separate comment-preserving override deletion.
pub fn set_terminal_setting(
    text: &str,
    edit: TerminalSettingEdit,
) -> Result<String, SettingsError> {
    let mut expected = SettingsPatch::parse(text)?;
    let table_name = match edit {
        TerminalSettingEdit::UiAppearance(_)
        | TerminalSettingEdit::UiAccent(_)
        | TerminalSettingEdit::UiLanguage(_) => "ui",
        _ => "terminal",
    };
    let (key, replacement) = match edit {
        TerminalSettingEdit::HistoryLimit(value) => {
            expected.terminal.history_limit = Some(value);
            ("history_limit", Value::from(i64::from(value)))
        }
        TerminalSettingEdit::UiAppearance(value) => {
            expected.ui.appearance = Some(value);
            ("appearance", Value::from(value.as_str()))
        }
        TerminalSettingEdit::UiLanguage(value) => {
            expected.ui.language = Some(value);
            ("language", Value::from(value.as_str()))
        }
        TerminalSettingEdit::UiAccent(value) => {
            expected.ui.accent = Some(value);
            ("accent", Value::from(value.as_str()))
        }
        TerminalSettingEdit::Theme(value) => {
            expected.terminal.theme = Some(value);
            let mut table = toml_edit::InlineTable::new();
            table.insert(
                "mode",
                Value::from(match value.mode {
                    crate::settings::TerminalThemeMode::Fixed => "fixed",
                    crate::settings::TerminalThemeMode::System => "system",
                }),
            );
            table.insert("light", Value::from(value.light.as_str()));
            table.insert("dark", Value::from(value.dark.as_str()));
            ("theme", Value::InlineTable(table))
        }
        TerminalSettingEdit::ColorPreset(value) => {
            expected.terminal.color_preset = Some(value);
            ("color_preset", Value::from(value.as_str()))
        }
        TerminalSettingEdit::FontSize(value) => {
            expected.terminal.font_size = Some(value);
            ("font_size", Value::from(i64::from(value)))
        }
        TerminalSettingEdit::LineHeightPercent(value) => {
            expected.terminal.line_height_percent = Some(value);
            ("line_height_percent", Value::from(i64::from(value)))
        }
        TerminalSettingEdit::LetterSpacing(value) => {
            expected.terminal.letter_spacing = Some(value);
            ("letter_spacing", Value::from(i64::from(value)))
        }
        TerminalSettingEdit::FontFamily(value) => {
            expected.terminal.font_family = Some(value.clone());
            ("font_family", Value::from(value))
        }
        TerminalSettingEdit::FontFallback(value) => {
            expected.terminal.font_fallback = Some(value.clone());
            ("font_fallback", Value::from(value))
        }
        TerminalSettingEdit::FontLigatures(value) => {
            expected.terminal.font_ligatures = Some(value);
            ("font_ligatures", Value::from(value))
        }
        TerminalSettingEdit::ScrollOnInput(value) => {
            expected.terminal.scroll_on_input = Some(value);
            ("scroll_on_input", Value::from(value))
        }
        TerminalSettingEdit::ScrollOnOutput(value) => {
            expected.terminal.scroll_on_output = Some(value);
            ("scroll_on_output", Value::from(value))
        }
        TerminalSettingEdit::MacOptionAsMeta(value) => {
            expected.terminal.mac_option_as_meta = Some(value);
            ("mac_option_as_meta", Value::from(value))
        }
        TerminalSettingEdit::DeleteSend(value) => {
            expected.terminal.delete_send = Some(value.clone());
            ("delete_send", Value::from(value))
        }
        TerminalSettingEdit::TermType(value) => {
            expected.terminal.term_type = Some(value);
            ("term_type", Value::from(value.as_str()))
        }
        TerminalSettingEdit::BackspaceSend(value) => {
            expected.terminal.backspace_send = Some(value.clone());
            ("backspace_send", Value::from(value))
        }
        TerminalSettingEdit::CursorBlink(value) => {
            expected.terminal.cursor_blink = Some(value);
            ("cursor_blink", Value::from(value))
        }
        TerminalSettingEdit::CopyOnSelect(value) => {
            expected.terminal.copy_on_select = Some(value);
            ("copy_on_select", Value::from(value))
        }
        TerminalSettingEdit::RightClickPaste(value) => {
            expected.terminal.right_click_paste = Some(value);
            ("right_click_paste", Value::from(value))
        }
        TerminalSettingEdit::ConfirmClose(value) => {
            expected.terminal.confirm_close = Some(value);
            ("confirm_close", Value::from(value))
        }
    };
    expected.validate()?;
    let raw = Document::parse(text).map_err(|_| SettingsError::Document)?;
    let parent = raw.as_table().get(table_name);
    let existing = parent
        .and_then(Item::as_table_like)
        .and_then(|table| table.get(key));
    let result = if let Some(existing) = existing {
        if let Some(table) = existing.as_table() {
            // Structured theme tables use the parser's exact leaf-value ranges.
            let replacement = replacement
                .as_inline_table()
                .ok_or(SettingsError::Document)?;
            let mut edits = Vec::new();
            for (key, value) in replacement.iter() {
                let span = table
                    .get(key)
                    .and_then(Item::as_value)
                    .and_then(Value::span)
                    .ok_or(SettingsError::Document)?;
                edits.push((span, value.to_string()));
            }
            edits.sort_by_key(|(span, _)| span.start);
            let mut result = text.to_owned();
            for (span, value) in edits.into_iter().rev() {
                result.get(span.clone()).ok_or(SettingsError::Document)?;
                result.replace_range(span, &value);
            }
            result
        } else {
            // Preserve every byte outside the immutable parser's value token.
            let span = existing
                .as_value()
                .and_then(Value::span)
                .ok_or(SettingsError::Document)?;
            let prefix = text.get(..span.start).ok_or(SettingsError::Document)?;
            let suffix = text.get(span.end..).ok_or(SettingsError::Document)?;
            format!("{prefix}{replacement}{suffix}")
        }
    } else if parent
        .and_then(Item::as_table)
        .is_some_and(Table::is_dotted)
    {
        let separator = if text.is_empty() || text.ends_with('\n') {
            ""
        } else {
            "\n"
        };
        format!("{text}{separator}{table_name}.{key} = {replacement}\n")
    } else {
        let mut document: DocumentMut = text.parse().map_err(|_| SettingsError::Document)?;
        if !document.contains_key(table_name) {
            document.insert(table_name, Item::Table(Table::new()));
        }
        let table = document
            .get_mut(table_name)
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
