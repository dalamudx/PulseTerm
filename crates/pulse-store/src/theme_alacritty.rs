use super::{Rgb8, TerminalPalette, TerminalScheme, ThemeError, budget, validate_name};
use serde::Deserialize;
#[derive(Clone, Copy, Deserialize)]
#[serde(try_from = "String")]
struct Color(Rgb8);
impl TryFrom<String> for Color {
    type Error = ThemeError;
    fn try_from(text: String) -> Result<Self, Self::Error> {
        // Alacritty also accepts 0xRRGGBB. Dynamic CellForeground/Background cannot be flattened.
        let normalized = text.strip_prefix("0x").map(|tail| format!("#{tail}"));
        Ok(Self(Rgb8::parse(normalized.as_deref().unwrap_or(&text))?))
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    colors: Colors,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Colors {
    primary: Primary,
    normal: Ansi,
    bright: Ansi,
    #[serde(default)]
    cursor: Option<Cursor>,
    #[serde(default)]
    selection: Option<Selection>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Primary {
    foreground: Color,
    background: Color,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ansi {
    black: Color,
    red: Color,
    green: Color,
    yellow: Color,
    blue: Color,
    magenta: Color,
    cyan: Color,
    white: Color,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    #[serde(default)]
    cursor: Option<Color>,
    #[serde(default)]
    text: Option<Color>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Selection {
    #[serde(default)]
    background: Option<Color>,
    #[serde(default)]
    text: Option<Color>,
}
/// Import a complete Alacritty color-only TOML fragment using an explicit display name.
/// Accepts #RRGGBB/0xRRGGBB; dynamic colors, indexed/dim extensions and other config fail closed.
/// Does not invent inherited colors, read files, apply a theme, or use the name as a path.
pub fn import_alacritty(name: &str, text: &str) -> Result<TerminalScheme, ThemeError> {
    budget(text)?;
    validate_name(name)?; // Before allocating an externally supplied display name.
    let w: Wire = toml::from_str(text).map_err(|_| ThemeError::Alacritty)?;
    let c = w.colors;
    let n = c.normal;
    let b = c.bright;
    TerminalScheme::validated(
        name.to_owned(),
        TerminalPalette {
            foreground: c.primary.foreground.0,
            background: c.primary.background.0,
            black: n.black.0,
            red: n.red.0,
            green: n.green.0,
            yellow: n.yellow.0,
            blue: n.blue.0,
            purple: n.magenta.0,
            cyan: n.cyan.0,
            white: n.white.0,
            bright_black: b.black.0,
            bright_red: b.red.0,
            bright_green: b.green.0,
            bright_yellow: b.yellow.0,
            bright_blue: b.blue.0,
            bright_purple: b.magenta.0,
            bright_cyan: b.cyan.0,
            bright_white: b.white.0,
            cursor: c.cursor.as_ref().and_then(|v| v.cursor).map(|v| v.0),
            cursor_text: c.cursor.and_then(|v| v.text).map(|v| v.0),
            selection_background: c.selection.as_ref().and_then(|v| v.background).map(|v| v.0),
            selection_foreground: c.selection.and_then(|v| v.text).map(|v| v.0),
        },
    )
}
#[cfg(test)]
#[path = "theme_alacritty_tests.rs"]
mod tests;
