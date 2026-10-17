use super::{Rgb8, TerminalPalette, TerminalScheme, ThemeError, budget, validate_name};
fn color(text: &str) -> Result<Rgb8, ThemeError> {
    if text.starts_with('#') {
        Rgb8::parse(text).map_err(|_| ThemeError::Ghostty)
    } else if text.len() == 6 && text.as_bytes().iter().all(u8::is_ascii_hexdigit) {
        Rgb8::parse(&format!("#{text}")).map_err(|_| ThemeError::Ghostty)
    } else {
        Err(ThemeError::Ghostty)
    }
}
/// Import a complete Ghostty color-only subset with an explicit display name.
/// Accepts unquoted RRGGBB/#RRGGBB, ANSI palette 0..15, foreground/background and optional
/// cursor/selection colors. Blank lines and standalone # comments are accepted. Unknown
/// directives, duplicates, extended indices and dynamic colors reject; no includes or I/O.
pub fn import_ghostty(name: &str, text: &str) -> Result<TerminalScheme, ThemeError> {
    budget(text)?;
    validate_name(name)?;
    let mut slots = [None; 22];
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (key, value) = line.split_once('=').ok_or(ThemeError::Ghostty)?;
        let value = value.trim();
        let (slot, value) = match key.trim() {
            "palette" => {
                let (index, v) = value.split_once('=').ok_or(ThemeError::Ghostty)?;
                let index = index.trim();
                if index.is_empty()
                    || index.len() > 2
                    || !index.as_bytes().iter().all(u8::is_ascii_digit)
                {
                    return Err(ThemeError::Ghostty);
                }
                let index = index.parse::<usize>().map_err(|_| ThemeError::Ghostty)?;
                if index > 15 {
                    return Err(ThemeError::Ghostty);
                }
                (index, v.trim())
            }
            "foreground" => (16, value),
            "background" => (17, value),
            "cursor-color" => (18, value),
            "cursor-text" => (19, value),
            "selection-background" => (20, value),
            "selection-foreground" => (21, value),
            _ => return Err(ThemeError::Ghostty),
        };
        if slots[slot].is_some() {
            return Err(ThemeError::Ghostty);
        }
        slots[slot] = Some(color(value)?);
    }
    let required = |i: usize| slots[i].ok_or(ThemeError::Ghostty);
    TerminalScheme::validated(
        name.to_owned(),
        TerminalPalette {
            foreground: required(16)?,
            background: required(17)?,
            black: required(0)?,
            red: required(1)?,
            green: required(2)?,
            yellow: required(3)?,
            blue: required(4)?,
            purple: required(5)?,
            cyan: required(6)?,
            white: required(7)?,
            bright_black: required(8)?,
            bright_red: required(9)?,
            bright_green: required(10)?,
            bright_yellow: required(11)?,
            bright_blue: required(12)?,
            bright_purple: required(13)?,
            bright_cyan: required(14)?,
            bright_white: required(15)?,
            cursor: slots[18],
            cursor_text: slots[19],
            selection_background: slots[20],
            selection_foreground: slots[21],
        },
    )
}
#[cfg(test)]
#[path = "theme_ghostty_tests.rs"]
mod tests;
