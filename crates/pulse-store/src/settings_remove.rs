//! Pure supported-override removal. No file IO, formatter, reset or arbitrary keys.
use crate::settings::{SettingsError, SettingsPatch};
use std::ops::Range;
use toml_edit::{Document, Value};
/// A caller may only remove one of the supported display settings.
#[derive(Debug, Clone, Copy)]
pub enum TerminalSettingKey {
    /// Remove the retention override.
    HistoryLimit,
    /// Remove the fixed terminal base override.
    ColorPreset,
    /// Remove the user-level system-theme policy.
    Theme,
    /// Remove the font-size override.
    FontSize,
    /// Remove the cell-height percentage override.
    LineHeightPercent,
    /// Remove the additional cell-width override, including explicit zero.
    LetterSpacing,
    /// Remove the font-family override.
    FontFamily,
    /// Remove the independently inherited fallback family.
    FontFallback,
    /// Remove an explicit on/off shaping override.
    FontLigatures,
    /// Remove the blink override, including an explicit false.
    CursorBlink,
    /// Remove an explicit return-to-bottom preference.
    ScrollOnInput,
    /// Force the live viewport after parsed output; default false.
    ScrollOnOutput,
    /// Remove the macOS Option-as-Meta override, including explicit false.
    MacOptionAsMeta,
    /// Remove the plain Backspace encoding override.
    BackspaceSend,
    /// Remove the Delete override.
    DeleteSend,
    /// Remove the TERM advertisement override.
    TermType,
    /// Remove the selection copy override, including explicit false.
    CopyOnSelect,
    /// Remove the right-click paste override.
    RightClickPaste,
    /// Remove only the user interface appearance override.
    UiAppearance,
    /// Remove the user-level UI accent override.
    UiAccent,
    /// Remove the user-level language override.
    UiLanguage,
}
/// Remove only the selected assignment and one necessary inline delimiter.
/// Keep comments, indentation, empty tables and every other byte. Validate both ends.
pub fn remove_terminal_setting(
    text: &str,
    key: TerminalSettingKey,
) -> Result<String, SettingsError> {
    let mut expected = SettingsPatch::parse(text)?;
    let table_name = match key {
        TerminalSettingKey::UiAppearance
        | TerminalSettingKey::UiAccent
        | TerminalSettingKey::UiLanguage => "ui",
        _ => "terminal",
    };
    let name = match key {
        TerminalSettingKey::Theme => {
            expected.terminal.theme = None;
            "theme"
        }
        TerminalSettingKey::HistoryLimit => {
            expected.terminal.history_limit = None;
            "history_limit"
        }
        TerminalSettingKey::UiAppearance => {
            expected.ui.appearance = None;
            "appearance"
        }
        TerminalSettingKey::UiLanguage => {
            expected.ui.language = None;
            "language"
        }
        TerminalSettingKey::UiAccent => {
            expected.ui.accent = None;
            "accent"
        }
        TerminalSettingKey::ColorPreset => {
            expected.terminal.color_preset = None;
            "color_preset"
        }
        TerminalSettingKey::FontSize => {
            expected.terminal.font_size = None;
            "font_size"
        }
        TerminalSettingKey::LineHeightPercent => {
            expected.terminal.line_height_percent = None;
            "line_height_percent"
        }
        TerminalSettingKey::LetterSpacing => {
            expected.terminal.letter_spacing = None;
            "letter_spacing"
        }
        TerminalSettingKey::FontFamily => {
            expected.terminal.font_family = None;
            "font_family"
        }
        TerminalSettingKey::FontFallback => {
            expected.terminal.font_fallback = None;
            "font_fallback"
        }
        TerminalSettingKey::FontLigatures => {
            expected.terminal.font_ligatures = None;
            "font_ligatures"
        }
        TerminalSettingKey::ScrollOnInput => {
            expected.terminal.scroll_on_input = None;
            "scroll_on_input"
        }
        TerminalSettingKey::ScrollOnOutput => {
            expected.terminal.scroll_on_output = None;
            "scroll_on_output"
        }
        TerminalSettingKey::MacOptionAsMeta => {
            expected.terminal.mac_option_as_meta = None;
            "mac_option_as_meta"
        }
        TerminalSettingKey::DeleteSend => {
            expected.terminal.delete_send = None;
            "delete_send"
        }
        TerminalSettingKey::TermType => {
            expected.terminal.term_type = None;
            "term_type"
        }
        TerminalSettingKey::BackspaceSend => {
            expected.terminal.backspace_send = None;
            "backspace_send"
        }
        TerminalSettingKey::CursorBlink => {
            expected.terminal.cursor_blink = None;
            "cursor_blink"
        }
        TerminalSettingKey::CopyOnSelect => {
            expected.terminal.copy_on_select = None;
            "copy_on_select"
        }
        TerminalSettingKey::RightClickPaste => {
            expected.terminal.right_click_paste = None;
            "right_click_paste"
        }
    };
    let raw = Document::parse(text).map_err(|_| SettingsError::Document)?;
    let Some(parent) = raw.as_table().get(table_name) else {
        return Ok(text.to_owned());
    };
    let table = parent.as_table_like().ok_or(SettingsError::Document)?;
    let Some((key, item)) = table.get_key_value(name) else {
        return Ok(text.to_owned());
    };
    let key_span = key.span().ok_or(SettingsError::Document)?;
    if let Some(theme) = item.as_table() {
        let mut ranges = Vec::new();
        if !theme.is_dotted() {
            // The parent key identifies this explicit section header.
            let line = text
                .get(..key_span.start)
                .ok_or(SettingsError::Document)?
                .rfind('\n')
                .map_or(0, |pos| pos + 1);
            let close = text
                .get(key_span.end..)
                .ok_or(SettingsError::Document)?
                .find(']')
                .ok_or(SettingsError::Document)?;
            ranges.push(line..key_span.end + close + 1);
        }
        for (keys, value) in theme.get_values() {
            let start = keys
                .first()
                .and_then(|key| key.span())
                .ok_or(SettingsError::Document)?
                .start;
            let prefix = text.get(..start).ok_or(SettingsError::Document)?;
            let line = prefix.rfind('\n').map_or(0, |pos| pos + 1);
            let indent = prefix
                .get(line..)
                .ok_or(SettingsError::Document)?
                .bytes()
                .take_while(|byte| matches!(byte, b' ' | b'\t'))
                .count();
            ranges.push(line + indent..value.span().ok_or(SettingsError::Document)?.end);
        }
        let result = splice(text, ranges)?;
        if SettingsPatch::parse(&result)? != expected {
            return Err(SettingsError::Document);
        }
        return Ok(result);
    }
    let value_span = item
        .as_value()
        .and_then(Value::span)
        .ok_or(SettingsError::Document)?;
    let mut ranges = Vec::with_capacity(2);
    if let Some(inline) = parent.as_value().and_then(Value::as_inline_table) {
        let outer = inline.span().ok_or(SettingsError::Document)?;
        let mut after = outer.end.checked_sub(1).ok_or(SettingsError::Document)?;
        let mut before = None;
        for (keys, value) in table.get_values() {
            let start = keys
                .first()
                .and_then(|key| key.span())
                .ok_or(SettingsError::Document)?
                .start;
            let end = value.span().ok_or(SettingsError::Document)?.end;
            if start > value_span.end {
                after = after.min(start);
            }
            if end < key_span.start {
                before = Some(before.map_or(end, |old: usize| old.max(end)));
            }
        }
        if let Some(comma) = trivia_comma(text, value_span.end..after)? {
            ranges.push(comma..comma + 1);
        } else if let Some(before) = before {
            let comma =
                trivia_comma(text, before..key_span.start)?.ok_or(SettingsError::Document)?;
            ranges.push(comma..comma + 1);
        }
        ranges.push(key_span.start..value_span.end);
    } else {
        // The parser establishes the key/value spans. A normal or dotted assignment
        // begins on its key's physical line; retain leading indentation and suffix decor.
        let prefix = text.get(..key_span.start).ok_or(SettingsError::Document)?;
        let line = prefix.rfind('\n').map_or(0, |pos| pos + 1);
        let indentation = prefix
            .get(line..)
            .ok_or(SettingsError::Document)?
            .bytes()
            .take_while(|byte| matches!(byte, b' ' | b'\t'))
            .count();
        ranges.push(line + indentation..value_span.end);
    }
    let result = splice(text, ranges)?;
    if SettingsPatch::parse(&result)? != expected {
        return Err(SettingsError::Document);
    }
    Ok(result)
}
// Only scan parser-bounded inter-entry trivia, never TOML keys, strings or values.
fn trivia_comma(text: &str, range: Range<usize>) -> Result<Option<usize>, SettingsError> {
    let bytes = text
        .get(range.clone())
        .ok_or(SettingsError::Document)?
        .as_bytes();
    let mut comma = None;
    let mut index = 0;
    while let Some(byte) = bytes.get(index) {
        match byte {
            b' ' | b'\t' | b'\r' | b'\n' => index += 1,
            b'#' => {
                while bytes.get(index).is_some_and(|byte| *byte != b'\n') {
                    index += 1;
                }
            }
            b',' => {
                if comma.replace(range.start + index).is_some() {
                    return Err(SettingsError::Document);
                }
                index += 1;
            }
            _ => return Err(SettingsError::Document),
        }
    }
    Ok(comma)
}
fn splice(text: &str, mut ranges: Vec<Range<usize>>) -> Result<String, SettingsError> {
    ranges.sort_by_key(|range| range.start);
    let mut result = String::with_capacity(text.len());
    let mut cursor = 0;
    for range in ranges {
        if range.start < cursor || range.start > range.end {
            return Err(SettingsError::Document);
        }
        result.push_str(
            text.get(cursor..range.start)
                .ok_or(SettingsError::Document)?,
        );
        text.get(range.clone()).ok_or(SettingsError::Document)?;
        cursor = range.end;
    }
    result.push_str(text.get(cursor..).ok_or(SettingsError::Document)?);
    Ok(result)
}
#[cfg(test)]
#[path = "settings_remove_tests.rs"]
mod tests;
