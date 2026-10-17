use super::{
    Rgb8, TerminalPalette, TerminalScheme, ThemeError, ThemeImport, budget, validate_name,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
/// Explicit appearance. System-following selection is managed outside each theme document.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeAppearance {
    /// Radix light scales.
    Light,
    /// Radix dark scales.
    Dark,
}
/// Supported Radix neutral families, not arbitrary CSS identifiers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NeutralScale {
    /// Gray neutral.
    Gray,
    /// Mauve neutral.
    Mauve,
    /// Slate neutral.
    Slate,
    /// Sage neutral.
    Sage,
    /// Olive neutral.
    Olive,
    /// Sand neutral.
    Sand,
}
const SCALES: [&str; 31] = [
    "gray", "mauve", "slate", "sage", "olive", "sand", "tomato", "red", "ruby", "crimson", "pink",
    "plum", "purple", "violet", "iris", "indigo", "blue", "cyan", "teal", "jade", "green", "grass",
    "brown", "orange", "amber", "yellow", "lime", "mint", "sky", "gold", "bronze",
];
/// Validated Radix accent family or an opaque RGB8 seed. No CSS expressions or URLs.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct AccentChoice(String);
impl AccentChoice {
    /// Select a known lowercase family or #RRGGBB seed; no whitespace normalization.
    pub fn parse(text: &str) -> Result<Self, ThemeError> {
        if text.len() > 7 && !SCALES.contains(&text) {
            return Err(ThemeError::Document);
        }
        if text.starts_with('#') {
            let c = Rgb8::try_from(text.to_owned()).map_err(|_| ThemeError::Document)?;
            return Ok(Self(String::from(c)));
        }
        if !SCALES.contains(&text) {
            return Err(ThemeError::Document);
        }
        Ok(Self(text.into()))
    }
    /// Validated family name or canonical hex seed. Consumers must not interpolate as raw CSS.
    pub fn value(&self) -> &str {
        &self.0
    }
}
impl TryFrom<String> for AccentChoice {
    type Error = ThemeError;
    fn try_from(s: String) -> Result<Self, Self::Error> {
        Self::parse(&s)
    }
}
impl From<AccentChoice> for String {
    fn from(a: AccentChoice) -> Self {
        a.0
    }
}
/// Explicit UI base, independent from terminal colors. This type does not generate CSS scales.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct UiThemeBase {
    neutral: NeutralScale,
    accent: AccentChoice,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    overrides: BTreeMap<String, Rgb8>,
}
impl UiThemeBase {
    /// Assemble a base with no imported overrides; imported overrides are added by ThemeDocument.
    pub fn new(neutral: NeutralScale, accent: AccentChoice) -> Self {
        Self {
            neutral,
            accent,
            overrides: BTreeMap::new(),
        }
    }
    /// Chosen neutral scale.
    pub fn neutral(&self) -> NeutralScale {
        self.neutral
    }
    /// Chosen accent family or seed; seed-to-scale generation is not implemented here.
    pub fn accent(&self) -> &AccentChoice {
        &self.accent
    }
    /// Fixed semantic colors; no font, spacing, or arbitrary custom properties.
    pub fn overrides(&self) -> &BTreeMap<String, Rgb8> {
        &self.overrides
    }
    fn validate(&self) -> Result<(), ThemeError> {
        let normal = [
            "--bg-base",
            "--bg-subtle",
            "--bg-surface",
            "--bg-hover",
            "--bg-active",
            "--border-subtle",
            "--border",
            "--border-strong",
            "--fg-muted",
            "--fg",
            "--accent-bg",
            "--accent-bg-hover",
            "--accent",
            "--accent-hover",
            "--accent-contrast",
            "--accent-fg",
        ];
        if self.overrides.len() > 28
            || self.overrides.keys().any(|key| {
                !normal.contains(&key.as_str())
                    && !matches!(
                        key.as_str(),
                        "--danger-bg"
                            | "--danger"
                            | "--danger-fg"
                            | "--warning-bg"
                            | "--warning"
                            | "--warning-fg"
                            | "--success-bg"
                            | "--success"
                            | "--success-fg"
                            | "--info-bg"
                            | "--info"
                            | "--info-fg"
                    )
            })
        {
            return Err(ThemeError::Document);
        }
        Ok(())
    }
}
/// Validated complete document structure, not a filename, applied stylesheet, or save authorization.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ThemeDocument {
    name: String,
    appearance: ThemeAppearance,
    ui: UiThemeBase,
    terminal: TerminalPalette,
}
impl ThemeDocument {
    /// Pair a validated import with explicit appearance/base. Preserves all fixed VS Code overrides.
    pub fn from_import(
        import: &ThemeImport,
        appearance: ThemeAppearance,
        ui: UiThemeBase,
    ) -> Result<Self, ThemeError> {
        if import.notes().base_palette_projection {
            return Err(ThemeError::Projection);
        }
        Self::from_projected_import(import, appearance, ui)
    }
    /// Explicitly accept an ANSI16 application projection. Extra source base colors are not saved;
    /// they remain in the import's base_scheme(). Unprojected imports retain their full semantics.
    pub fn from_projected_import(
        import: &ThemeImport,
        appearance: ThemeAppearance,
        mut ui: UiThemeBase,
    ) -> Result<Self, ThemeError> {
        if let Some(overrides) = import.ui_overrides() {
            for (k, v) in overrides {
                ui.overrides.insert((*k).to_owned(), *v);
            }
        }
        ui.validate()?;
        Ok(Self {
            name: import.terminal().name().into(),
            appearance,
            ui,
            terminal: import.terminal().palette().clone(),
        })
    }
    /// Imported literal display name, not a path.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Explicit light/dark choice.
    pub fn appearance(&self) -> ThemeAppearance {
        self.appearance
    }
    /// UI base and validated fixed semantic overrides.
    pub fn ui(&self) -> &UiThemeBase {
        &self.ui
    }
    /// Independently selectable terminal-only palette.
    pub fn terminal(&self) -> &TerminalPalette {
        &self.terminal
    }
    /// Project complete document to a terminal-only display scheme, without losing the original document.
    pub fn terminal_scheme(&self) -> Result<TerminalScheme, ThemeError> {
        validate_name(&self.name)?;
        Ok(TerminalScheme {
            name: self.name.clone(),
            terminal: self.terminal.clone(),
        })
    }
    /// Fixed rendering data only; excludes name/path/permissions. Does not resolve Radix scales or apply styles.
    pub fn paint_data(&self) -> Result<super::ThemePaintData, ThemeError> {
        super::ThemePaintData::from_document(self)
    }
    /// Bounded canonical .theme.toml contents. Does not write a file or apply styles.
    pub fn to_toml(&self) -> Result<String, ThemeError> {
        let text = toml::to_string(self).map_err(|_| ThemeError::Encoding)?;
        budget(&text)?;
        Ok(text)
    }
}
/// Parse a complete bounded document; fragments, invalid scale names and arbitrary CSS reject.
pub fn parse_theme_document(text: &str) -> Result<ThemeDocument, ThemeError> {
    budget(text)?;
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Wire {
        name: String,
        appearance: ThemeAppearance,
        ui: UiWire,
        terminal: TerminalPalette,
    }
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct UiWire {
        neutral: NeutralScale,
        accent: AccentChoice,
        #[serde(default)]
        overrides: BTreeMap<String, Rgb8>,
    }
    let w: Wire = toml::from_str(text).map_err(|_| ThemeError::Document)?;
    let ui = UiThemeBase {
        neutral: w.ui.neutral,
        accent: w.ui.accent,
        overrides: w.ui.overrides,
    };
    ui.validate()?;
    validate_name(&w.name)?;
    let s = TerminalScheme {
        name: w.name,
        terminal: w.terminal,
    };
    Ok(ThemeDocument {
        name: s.name().into(),
        appearance: w.appearance,
        ui,
        terminal: s.palette().clone(),
    })
}
#[cfg(test)]
#[path = "theme_document_tests.rs"]
mod tests;
