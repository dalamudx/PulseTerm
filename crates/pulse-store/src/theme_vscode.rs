use super::{Rgb8, TerminalPalette, TerminalScheme, ThemeError, budget, validate_name};
use serde::{
    Deserialize, Serialize,
    de::{self, MapAccess, Visitor},
};
use std::{collections::BTreeMap, fmt};
const ANSI: [&str; 16] = [
    "terminal.ansiBlack",
    "terminal.ansiRed",
    "terminal.ansiGreen",
    "terminal.ansiYellow",
    "terminal.ansiBlue",
    "terminal.ansiMagenta",
    "terminal.ansiCyan",
    "terminal.ansiWhite",
    "terminal.ansiBrightBlack",
    "terminal.ansiBrightRed",
    "terminal.ansiBrightGreen",
    "terminal.ansiBrightYellow",
    "terminal.ansiBrightBlue",
    "terminal.ansiBrightMagenta",
    "terminal.ansiBrightCyan",
    "terminal.ansiBrightWhite",
];
const UI: [(&str, &str); 6] = [
    ("editor.background", "--bg-base"),
    ("sideBar.background", "--bg-subtle"),
    ("editor.foreground", "--fg"),
    ("focusBorder", "--border-strong"),
    ("button.background", "--accent"),
    ("button.foreground", "--accent-contrast"),
];
struct Colors(BTreeMap<String, String>);
impl<'de> Deserialize<'de> for Colors {
    fn deserialize<D: de::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl<'de> Visitor<'de> for V {
            type Value = Colors;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("unique color string map")
            }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> Result<Colors, M::Error> {
                let mut colors = BTreeMap::new();
                while let Some((k, v)) = map.next_entry::<String, String>()? {
                    if colors.len() >= 512 || colors.insert(k, v).is_some() {
                        return Err(de::Error::custom("invalid color map"));
                    }
                }
                Ok(Colors(colors))
            }
        }
        d.deserialize_map(V)
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Wire {
    name: String,
    colors: Colors,
    #[serde(rename = "$schema", default)]
    _schema: Option<String>,
    #[serde(rename = "type", default)]
    _kind: Option<String>,
    #[serde(rename = "semanticHighlighting", default)]
    _semantic: Option<bool>,
    #[serde(rename = "tokenColors", default)]
    _tokens: Option<de::IgnoredAny>,
    #[serde(rename = "semanticTokenColors", default)]
    _semantic_tokens: Option<de::IgnoredAny>,
}
/// Bounded extraction, not a full application theme or token grammar. No file reads or includes.
/// Terminal palette and fixed UI overrides are extracted together. Other editor colors are
/// explicitly counted as ignored; unknown terminal colors reject rather than silently disappear.
#[derive(Clone, PartialEq, Eq)]
pub struct VsCodeTheme {
    terminal: TerminalScheme,
    ui: BTreeMap<&'static str, Rgb8>,
    ignored_colors: usize,
}
impl VsCodeTheme {
    /// Complete terminal palette with independently checked display name.
    pub fn terminal(&self) -> &TerminalScheme {
        &self.terminal
    }
    /// Only six fixed semantic color variables; never imported CSS variable names.
    pub fn ui_overrides(&self) -> &BTreeMap<&'static str, Rgb8> {
        &self.ui
    }
    /// Unmapped nonterminal editor color entries. Token grammars are never applied.
    pub fn ignored_color_count(&self) -> usize {
        self.ignored_colors
    }
    /// Sparse terminal + UI override fragment. UI base palette and appearance are not inferred.
    pub fn to_toml_fragment(&self) -> Result<String, ThemeError> {
        #[derive(Serialize)]
        struct Ui<'a> {
            overrides: &'a BTreeMap<&'static str, Rgb8>,
        }
        #[derive(Serialize)]
        struct Fragment<'a> {
            name: &'a str,
            terminal: &'a TerminalPalette,
            ui: Ui<'a>,
        }
        let text = toml::to_string(&Fragment {
            name: self.terminal.name(),
            terminal: self.terminal.palette(),
            ui: Ui {
                overrides: &self.ui,
            },
        })
        .map_err(|_| ThemeError::Encoding)?;
        budget(&text)?;
        Ok(text)
    }
}
fn depth(text: &str) -> Result<(), ThemeError> {
    let (mut quoted, mut escape, mut n) = (false, false, 0usize);
    for b in text.bytes() {
        if quoted {
            if escape {
                escape = false;
            } else if b == b'\\' {
                escape = true;
            } else if b == b'"' {
                quoted = false;
            }
        } else {
            match b {
                b'"' => quoted = true,
                b'{' | b'[' => {
                    n += 1;
                    if n > 32 {
                        return Err(ThemeError::VsCode);
                    }
                }
                b'}' | b']' => {
                    n = n.checked_sub(1).ok_or(ThemeError::VsCode)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}
/// Strict JSON (not JSONC) VS Code theme with complete ANSI16/foreground/background.
/// Opaque #RRGGBB only for extracted colors. Includes and unknown top-level fields reject.
/// Byte (64 KiB), depth (32) and color-count (512) limits apply before extraction.
pub fn import_vscode(text: &str) -> Result<VsCodeTheme, ThemeError> {
    budget(text)?;
    depth(text)?;
    let w: Wire = serde_json::from_str(text).map_err(|_| ThemeError::VsCode)?;
    validate_name(&w.name)?;
    let mut colors = w.colors.0;
    let mut take = |key: &str| -> Result<Option<Rgb8>, ThemeError> {
        colors
            .remove(key)
            .map(|v| Rgb8::parse(&v).map_err(|_| ThemeError::VsCode))
            .transpose()
    };
    let foreground = take("terminal.foreground")?.ok_or(ThemeError::VsCode)?;
    let background = take("terminal.background")?.ok_or(ThemeError::VsCode)?;
    let mut ansi = [Rgb8::new(0, 0, 0); 16];
    for (i, key) in ANSI.iter().enumerate() {
        ansi[i] = take(key)?.ok_or(ThemeError::VsCode)?;
    }
    let cursor = take("terminalCursor.foreground")?;
    let cursor_text = take("terminalCursor.background")?;
    let selection_background = take("terminal.selectionBackground")?;
    let selection_foreground = take("terminal.selectionForeground")?;
    let mut ui = BTreeMap::new();
    for (key, variable) in UI {
        if let Some(value) = take(key)? {
            ui.insert(variable, value);
        }
    }
    if colors
        .keys()
        .any(|k| k.starts_with("terminal.") || k.starts_with("terminalCursor."))
    {
        return Err(ThemeError::VsCode);
    }
    let terminal = TerminalScheme::validated(
        w.name,
        TerminalPalette {
            foreground,
            background,
            black: ansi[0],
            red: ansi[1],
            green: ansi[2],
            yellow: ansi[3],
            blue: ansi[4],
            purple: ansi[5],
            cyan: ansi[6],
            white: ansi[7],
            bright_black: ansi[8],
            bright_red: ansi[9],
            bright_green: ansi[10],
            bright_yellow: ansi[11],
            bright_blue: ansi[12],
            bright_purple: ansi[13],
            bright_cyan: ansi[14],
            bright_white: ansi[15],
            cursor,
            cursor_text,
            selection_background,
            selection_foreground,
        },
    )?;
    Ok(VsCodeTheme {
        terminal,
        ui,
        ignored_colors: colors.len(),
    })
}
#[cfg(test)]
#[path = "theme_vscode_tests.rs"]
mod tests;
