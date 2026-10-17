//! Bounded theme conversion. Imported names are display text, never paths or CSS.
//! Fragments are NOT complete PulseTerm UI themes; VS Code includes fixed UI overrides.
use serde::{Deserialize, Serialize};
use thiserror::Error;
#[path = "theme_alacritty.rs"]
mod alacritty;
pub use alacritty::import_alacritty;
#[path = "theme_ghostty.rs"]
mod ghostty;
pub use ghostty::import_ghostty;
#[path = "theme_base.rs"]
mod base;
pub use base::{BaseScheme, BaseSystem, import_base_scheme};
#[path = "theme_vscode.rs"]
mod vscode;
pub use vscode::{VsCodeTheme, import_vscode};
#[path = "theme_iterm.rs"]
mod iterm;
pub use iterm::import_itermcolors;
#[path = "theme_import.rs"]
mod import;
pub use import::{ThemeImport, ThemeImportFormat, ThemeImportNotes, import_theme};
#[path = "theme_document.rs"]
mod document;
#[path = "theme_paint.rs"]
mod paint;
pub use document::{
    AccentChoice, NeutralScale, ThemeAppearance, ThemeDocument, UiThemeBase, parse_theme_document,
};
pub use paint::{MAX_THEME_PAINT_BYTES, TerminalPaint, ThemePaintData, UiColorSlot, UiPaintColor};
/// Maximum UTF-8 byte size for a single scheme input or emitted terminal fragment.
pub const MAX_SCHEME_BYTES: usize = 64 * 1024;
const MAX_NAME_BYTES: usize = 128;
/// Content-free failures; parser errors and imported text are never included.
#[derive(Debug, Error, PartialEq, Eq)]
pub enum ThemeError {
    /// Input or output exceeds the fixed byte quota.
    #[error("theme.input_budget")]
    Budget,
    /// Color is not an opaque ASCII #RRGGBB value.
    #[error("theme.invalid_color")]
    Color,
    /// Display name is empty, oversized, or contains controls.
    #[error("theme.invalid_name")]
    Name,
    /// Strict complete Windows Terminal scheme parsing failed.
    #[error("theme.invalid_windows_terminal_json")]
    Json,
    /// Terminal-only TOML parsing failed; UI documents are not accepted.
    #[error("theme.invalid_terminal_fragment")]
    Toml,
    /// Strict Alacritty color-only TOML parsing failed, including unrepresentable colors.
    #[error("theme.invalid_alacritty_colors")]
    Alacritty,
    /// Strict Ghostty color-only text failed, including duplicate or unrepresentable values.
    #[error("theme.invalid_ghostty_colors")]
    Ghostty,
    /// Strict VS Code JSON extraction failed; includes, JSONC and dynamic colors are unsupported.
    #[error("theme.invalid_vscode_theme")]
    VsCode,
    /// Strict sRGB XML iTerm color subset failed; generic/device color spaces are unsupported.
    #[error("theme.invalid_itermcolors")]
    Iterm,
    /// Complete source base palette would be lost without explicit projection acceptance.
    #[error("theme.projection_requires_acceptance")]
    Projection,
    /// Base16/24 YAML fails the bounded scalar-only subset or expected system.
    #[error("theme.invalid_base_yaml")]
    BaseYaml,
    /// Complete theme document, base palette, or semantic override is invalid.
    #[error("theme.invalid_document")]
    Document,
    /// Embedded-name formats forbid an override; nameless formats require one.
    #[error("theme.invalid_import_name_policy")]
    NamePolicy,
    /// Validated terminal fragment could not be encoded.
    #[error("theme.encoding_failed")]
    Encoding,
}
/// Opaque RGB only: no CSS functions, named colors, alpha interpretation, or shell text.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct Rgb8([u8; 3]);
impl Rgb8 {
    /// Construct an opaque color from three byte channels.
    pub const fn new(red: u8, green: u8, blue: u8) -> Self {
        Self([red, green, blue])
    }
    /// Return red, green, and blue channels in that order.
    pub const fn channels(self) -> [u8; 3] {
        self.0
    }
    /// Accept exactly #RRGGBB; output serialization uses lowercase hex.
    pub fn parse(text: &str) -> Result<Self, ThemeError> {
        let b = text.as_bytes();
        if b.len() != 7 || b[0] != b'#' || !b[1..].iter().all(u8::is_ascii_hexdigit) {
            return Err(ThemeError::Color);
        }
        let channel = |i| u8::from_str_radix(&text[i..i + 2], 16).map_err(|_| ThemeError::Color);
        Ok(Self::new(channel(1)?, channel(3)?, channel(5)?))
    }
    fn luminance(self) -> f64 {
        let linear = self.0.map(|v| {
            let s = f64::from(v) / 255.0;
            if s <= 0.04045 {
                s / 12.92
            } else {
                ((s + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * linear[0] + 0.7152 * linear[1] + 0.0722 * linear[2]
    }
}
impl TryFrom<String> for Rgb8 {
    type Error = ThemeError;
    fn try_from(v: String) -> Result<Self, Self::Error> {
        Self::parse(&v)
    }
}
impl From<Rgb8> for String {
    fn from(v: Rgb8) -> Self {
        format!("#{:02x}{:02x}{:02x}", v.0[0], v.0[1], v.0[2])
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
/// Complete ANSI palette with optional cursor/selection colors, separate from UI colors.
pub struct TerminalPalette {
    /// Default text foreground.
    pub foreground: Rgb8,
    /// Default terminal background.
    pub background: Rgb8,
    /// ANSI slot 0.
    pub black: Rgb8,
    /// ANSI slot 1.
    pub red: Rgb8,
    /// ANSI slot 2.
    pub green: Rgb8,
    /// ANSI slot 3.
    pub yellow: Rgb8,
    /// ANSI slot 4.
    pub blue: Rgb8,
    /// ANSI slot 5 (magenta).
    pub purple: Rgb8,
    /// ANSI slot 6.
    pub cyan: Rgb8,
    /// ANSI slot 7.
    pub white: Rgb8,
    /// ANSI slot 8.
    pub bright_black: Rgb8,
    /// ANSI slot 9.
    pub bright_red: Rgb8,
    /// ANSI slot 10.
    pub bright_green: Rgb8,
    /// ANSI slot 11.
    pub bright_yellow: Rgb8,
    /// ANSI slot 12.
    pub bright_blue: Rgb8,
    /// ANSI slot 13 (bright magenta).
    pub bright_purple: Rgb8,
    /// ANSI slot 14.
    pub bright_cyan: Rgb8,
    /// ANSI slot 15.
    pub bright_white: Rgb8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional cursor background; imported from Windows Terminal cursorColor.
    pub cursor: Option<Rgb8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional cursor foreground; absent values are left unspecified.
    pub cursor_text: Option<Rgb8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional selection background.
    pub selection_background: Option<Rgb8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// Optional selection foreground.
    pub selection_foreground: Option<Rgb8>,
}
impl TerminalPalette {
    /// ANSI order; indices 5/13 are magenta (Windows Terminal calls these purple).
    pub fn ansi(&self) -> [Rgb8; 16] {
        [
            self.black,
            self.red,
            self.green,
            self.yellow,
            self.blue,
            self.purple,
            self.cyan,
            self.white,
            self.bright_black,
            self.bright_red,
            self.bright_green,
            self.bright_yellow,
            self.bright_blue,
            self.bright_purple,
            self.bright_cyan,
            self.bright_white,
        ]
    }
    /// Warning only: imported colors are never silently modified.
    pub fn contrast(&self) -> ContrastReport {
        let a = self.foreground.luminance();
        let b = self.background.luminance();
        let ratio = (a.max(b) + 0.05) / (a.min(b) + 0.05);
        ContrastReport {
            ratio,
            warning: (ratio < 4.5).then_some(ContrastWarning::LowForegroundContrast),
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
/// Advisory readability findings; not permission or an automatic palette adjustment.
pub enum ContrastWarning {
    /// Foreground/background WCAG ratio is strictly below 4.5:1.
    LowForegroundContrast,
}
#[derive(Clone, Copy, Debug, PartialEq)]
/// WCAG sRGB foreground/background comparison, using unrounded threshold checks.
pub struct ContrastReport {
    /// Finite symmetric ratio from 1 to 21, not rounded for comparison.
    pub ratio: f64,
    /// Advisory only; colors are kept exactly as supplied.
    pub warning: Option<ContrastWarning>,
}
#[derive(Clone, PartialEq, Eq, Serialize)]
/// Validated display name and terminal-only palette; never a filename or complete UI theme.
pub struct TerminalScheme {
    name: String,
    terminal: TerminalPalette,
}
impl TerminalScheme {
    /// Literal imported display name. Consumers must render as text, not HTML or a path.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Immutable validated terminal colors.
    pub fn palette(&self) -> &TerminalPalette {
        &self.terminal
    }
    /// Encode name/[terminal] only; does not write files or select UI appearance/colors.
    pub fn to_toml_fragment(&self) -> Result<String, ThemeError> {
        let out = toml::to_string(self).map_err(|_| ThemeError::Encoding)?;
        budget(&out)?;
        Ok(out)
    }
    fn validated(name: String, terminal: TerminalPalette) -> Result<Self, ThemeError> {
        validate_name(&name)?;
        Ok(Self { name, terminal })
    }
}
fn validate_name(name: &str) -> Result<(), ThemeError> {
    if name.is_empty()
        || name.len() > MAX_NAME_BYTES
        || name.trim().is_empty()
        || name.chars().any(char::is_control)
    {
        Err(ThemeError::Name)
    } else {
        Ok(())
    }
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct WindowsTerminalWire {
    name: String,
    foreground: Rgb8,
    background: Rgb8,
    black: Rgb8,
    red: Rgb8,
    green: Rgb8,
    yellow: Rgb8,
    blue: Rgb8,
    purple: Rgb8,
    cyan: Rgb8,
    white: Rgb8,
    bright_black: Rgb8,
    bright_red: Rgb8,
    bright_green: Rgb8,
    bright_yellow: Rgb8,
    bright_blue: Rgb8,
    bright_purple: Rgb8,
    bright_cyan: Rgb8,
    bright_white: Rgb8,
    #[serde(default)]
    cursor_color: Option<Rgb8>,
    #[serde(default)]
    selection_background: Option<Rgb8>,
}
fn budget(text: &str) -> Result<(), ThemeError> {
    if text.len() > MAX_SCHEME_BYTES {
        Err(ThemeError::Budget)
    } else {
        Ok(())
    }
}
/// A single complete strict JSON scheme, NOT settings.json/JSONC or a scheme catalog.
pub fn import_windows_terminal(text: &str) -> Result<TerminalScheme, ThemeError> {
    budget(text)?;
    let w: WindowsTerminalWire = serde_json::from_str(text).map_err(|_| ThemeError::Json)?;
    TerminalScheme::validated(
        w.name,
        TerminalPalette {
            foreground: w.foreground,
            background: w.background,
            black: w.black,
            red: w.red,
            green: w.green,
            yellow: w.yellow,
            blue: w.blue,
            purple: w.purple,
            cyan: w.cyan,
            white: w.white,
            bright_black: w.bright_black,
            bright_red: w.bright_red,
            bright_green: w.bright_green,
            bright_yellow: w.bright_yellow,
            bright_blue: w.bright_blue,
            bright_purple: w.bright_purple,
            bright_cyan: w.bright_cyan,
            bright_white: w.bright_white,
            cursor: w.cursor_color,
            cursor_text: None,
            selection_background: w.selection_background,
            selection_foreground: None,
        },
    )
}
/// Sparse UI/appearance/overrides are not accepted here: this is a terminal fragment only.
pub fn parse_terminal_fragment(text: &str) -> Result<TerminalScheme, ThemeError> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        name: String,
        terminal: TerminalPalette,
    }
    budget(text)?;
    let w: Wire = toml::from_str(text).map_err(|_| ThemeError::Toml)?;
    TerminalScheme::validated(w.name, w.terminal)
}
#[cfg(test)]
#[path = "theme_tests.rs"]
mod tests;
