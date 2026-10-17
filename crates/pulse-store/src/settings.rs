//! Validated terminal display and clipboard preferences, never executable policy.
use regex::Regex;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
#[path = "settings_ui.rs"]
mod ui;
use std::sync::LazyLock;
#[path = "settings_terminal_theme.rs"]
mod terminal_theme;
pub use terminal_theme::{TerminalTheme, TerminalThemeMode};
pub use ui::{UiAccent, UiAppearance, UiLanguage, UiPatch};
#[path = "settings_startup.rs"]
mod startup;
#[path = "settings_terminal_preset.rs"]
mod terminal_preset;
pub use startup::TerminalStartup;
#[path = "terminal_type.rs"]
mod terminal_type;
pub use terminal_preset::{TerminalPreset, TerminalPresetColors, TerminalPresetPaint};
pub use terminal_type::TerminalType;
/// Maximum bytes in each individual overlay before TOML parsing.
pub const SETTINGS_TEXT_LIMIT: usize = 65_536;
/// Single CSS-safe font-family name; no quoting, lists, URLs or declarations.
pub const FONT_NAME_PATTERN: &str = r"^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$";
static FONT_NAME: LazyLock<Option<Regex>> = LazyLock::new(|| Regex::new(FONT_NAME_PATTERN).ok());
/// Compiled defaults; not read from the user's filesystem.
pub const DEFAULTS_TOML: &str = include_str!("../defaults.toml");
/// Stable errors never contain document text, paths or parse-error messages.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SettingsError {
    /// A single input exceeds the pre-parsing byte budget.
    #[error("settings.document_too_large")]
    Size,
    /// TOML or a known setting has an invalid type or unknown fields.
    #[error("settings.invalid_document")]
    Document,
    /// Only the current overlay schema version is supported.
    #[error("settings.unsupported_version")]
    Version,
    /// A known value violates its numeric or safe-name bounds.
    #[error("settings.invalid_value")]
    Value,
    /// Compiled defaults or compiled validation expression are inconsistent.
    #[error("settings.invalid_defaults")]
    Defaults,
}
/// Sparse document; omitted fields inherit, false is an explicit override.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct SettingsPatch {
    /// Omission means schema 1; future versions cannot silently be accepted.
    #[schemars(range(min = 1, max = 1))]
    pub schema_version: Option<u16>,
    /// Initial terminal display settings only; no credential or command fields.
    pub terminal: TerminalPatch,
    /// User-level interface preferences, independent of terminal colors.
    pub ui: UiPatch,
}
/// Every field is optional, independently resolved and independently validated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalPatch {
    /// Atomic user-level policy; scoped fixed palettes override it.
    pub theme: Option<TerminalTheme>,
    /// Fixed base palette for new local sessions.
    pub color_preset: Option<TerminalPreset>,
    /// Maximum retained primary history rows; byte limits may trim earlier.
    #[schemars(range(min = 1000, max = 100000))]
    pub history_limit: Option<u32>,
    /// Initial supported font-size range, in CSS pixels, inclusive.
    #[schemars(range(min = 6, max = 72))]
    pub font_size: Option<u16>,
    /// Cell-height multiplier expressed as an integer percentage, 100 through 300.
    #[schemars(range(min = 100, max = 300))]
    pub line_height_percent: Option<u16>,
    /// Additional cell width in CSS pixels, zero through eight.
    #[schemars(range(min = 0, max = 8))]
    pub letter_spacing: Option<u16>,
    /// A single font name, at most 128 Unicode scalars, using safe characters.
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = r"^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$")
    )]
    pub font_family: Option<String>,
    /// One independently inherited fallback name with the primary name’s bounds.
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = r"^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$")
    )]
    pub font_fallback: Option<String>,
    /// Default-off drawing of programming-operator ligatures in the built-in font.
    pub font_ligatures: Option<bool>,
    /// Explicit false still replaces lower-priority true.
    pub cursor_blink: Option<bool>,
    /// Return to the live viewport after authorized input; display behavior only.
    pub scroll_on_input: Option<bool>,
    /// Force the live viewport after parsed output; default false.
    pub scroll_on_output: Option<bool>,
    /// Treat macOS Option as Meta in the renderer; default false.
    pub mac_option_as_meta: Option<bool>,
    /// Unmodified Backspace encoding: del (default) or bs.
    #[schemars(regex(pattern = "^(del|bs)$"))]
    pub backspace_send: Option<String>,
    /// Plain Delete uses csi (ESC[3~) or del (DEL).
    #[schemars(regex(pattern = "^(csi|del)$"))]
    pub delete_send: Option<String>,
    /// Terminal capability name advertised only at new-session startup.
    pub term_type: Option<TerminalType>,
    /// User selection copy preference; false by default, never triggered by terminal output.
    pub copy_on_select: Option<bool>,
    /// Explicit default-off right-click paste preference; no input authority.
    pub right_click_paste: Option<bool>,
}
impl SettingsPatch {
    /// Parse a bounded sparse overlay; all fields validate before any resolution.
    pub fn parse(text: &str) -> Result<Self, SettingsError> {
        if text.len() > SETTINGS_TEXT_LIMIT {
            return Err(SettingsError::Size);
        }
        let patch: Self = toml::from_str(text).map_err(|_| SettingsError::Document)?;
        patch.validate()?;
        Ok(patch)
    }
    /// Also enforce invariants on patches created directly by backend Rust callers.
    pub fn validate(&self) -> Result<(), SettingsError> {
        if self
            .terminal
            .history_limit
            .is_some_and(|v| !(1000..=100_000).contains(&v))
        {
            return Err(SettingsError::Value);
        }
        if self.schema_version.is_some_and(|version| version != 1) {
            return Err(SettingsError::Version);
        }
        if self
            .terminal
            .font_size
            .is_some_and(|size| !(6..=72).contains(&size))
        {
            return Err(SettingsError::Value);
        }
        if self
            .terminal
            .line_height_percent
            .is_some_and(|value| !(100..=300).contains(&value))
            || self.terminal.letter_spacing.is_some_and(|value| value > 8)
        {
            return Err(SettingsError::Value);
        }
        for name in [
            self.terminal.font_family.as_ref(),
            self.terminal.font_fallback.as_ref(),
        ]
        .into_iter()
        .flatten()
        {
            if name.len() > 512 || name.chars().count() > 128 {
                return Err(SettingsError::Value);
            }
            if !FONT_NAME
                .as_ref()
                .ok_or(SettingsError::Defaults)?
                .is_match(name)
            {
                return Err(SettingsError::Value);
            }
        }
        if self
            .terminal
            .delete_send
            .as_deref()
            .is_some_and(|value| !matches!(value, "csi" | "del"))
        {
            return Err(SettingsError::Value);
        }
        if self
            .terminal
            .backspace_send
            .as_deref()
            .is_some_and(|value| !matches!(value, "del" | "bs"))
        {
            return Err(SettingsError::Value);
        }
        Ok(())
    }
}
/// Provenance identifies a layer, not a file path, host identity or authorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SettingsSource {
    /// Included defaults.toml.
    Default,
    /// User settings overlay.
    User,
    /// One group overlay; nested group ancestry is not implemented.
    Group,
    /// One host overlay, higher priority than the group.
    Host,
}
/// The effective value and its own provenance always travel together.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Setting<T> {
    /// Validated effective value.
    pub value: T,
    /// Last layer explicitly supplying this field.
    pub source: SettingsSource,
}
/// Initial display-only result; it does not grant launch or credential access.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ResolvedSettings {
    /// Effective font size.
    pub font_size: Setting<u16>,
    /// Effective cell-height percentage and its own source.
    pub line_height_percent: Setting<u16>,
    /// Effective extra cell width in CSS pixels and its own source.
    pub letter_spacing: Setting<u16>,
    /// Effective safe single font family.
    pub font_family: Setting<String>,
    /// Effective bounded fallback family and its own source.
    pub font_fallback: Setting<String>,
    /// Default-off built-in operator shaping; drawing data only.
    pub font_ligatures: Setting<bool>,
    /// Effective cursor blink policy.
    pub cursor_blink: Setting<bool>,
    /// Whether authorized user input returns the view to the live viewport.
    pub scroll_on_input: Setting<bool>,
    /// Force the live viewport after parsed output; default false.
    pub scroll_on_output: Setting<bool>,
    /// Effective macOS Option-as-Meta preference; never input authority.
    pub mac_option_as_meta: Setting<bool>,
    /// Effective unmodified Backspace encoding; no input authority.
    pub backspace_send: Setting<String>,
    /// Plain Delete encoding, independent of Backspace.
    pub delete_send: Setting<String>,
    /// Frozen startup TERM advertisement for new sessions.
    pub term_type: Setting<TerminalType>,
    /// Copy displayed selection only after a trusted user gesture; not input authority.
    pub copy_on_select: Setting<bool>,
    /// Trusted right-click reads still require current view input and paste approval.
    pub right_click_paste: Setting<bool>,
    /// Interface appearance resolved from defaults and user preferences only.
    pub ui_appearance: Setting<UiAppearance>,
    /// Interface accent resolved independently; user-level only.
    pub ui_accent: Setting<UiAccent>,
    /// Interface language, resolved only from user preferences.
    pub ui_language: Setting<UiLanguage>,
    /// Atomic user-level light/dark selection policy.
    pub terminal_theme: Setting<TerminalTheme>,
    /// Fixed terminal base, separate from UI appearance.
    pub terminal_preset: Setting<TerminalPreset>,
    /// Retained hot and cold history rows for new sessions.
    pub history_limit: Setting<u32>,
}
/// Pure resolver; has no filesystem, SQLite, watcher or frontend command entry.
pub struct SettingsResolver;
impl SettingsResolver {
    /// Defaults < user < group < host, with invalid lower layers still rejected.
    pub fn resolve(
        user: Option<&str>,
        group: Option<&str>,
        host: Option<&str>,
    ) -> Result<ResolvedSettings, SettingsError> {
        let user = user.map(SettingsPatch::parse).transpose()?;
        let group = group.map(SettingsPatch::parse).transpose()?;
        let host = host.map(SettingsPatch::parse).transpose()?;
        Self::resolve_patches(user.as_ref(), group.as_ref(), host.as_ref())
    }
    /// Resolve validated sparse backend models with the same precedence as text layers.
    /// Every supplied layer is revalidated; this grants no host/window authority.
    pub fn resolve_patches(
        user: Option<&SettingsPatch>,
        group: Option<&SettingsPatch>,
        host: Option<&SettingsPatch>,
    ) -> Result<ResolvedSettings, SettingsError> {
        let defaults = SettingsPatch::parse(DEFAULTS_TOML)?;
        let mut result = ResolvedSettings {
            font_size: Setting {
                value: defaults.terminal.font_size.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            line_height_percent: Setting {
                value: defaults
                    .terminal
                    .line_height_percent
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            letter_spacing: Setting {
                value: defaults
                    .terminal
                    .letter_spacing
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            font_family: Setting {
                value: defaults
                    .terminal
                    .font_family
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            font_fallback: Setting {
                value: defaults
                    .terminal
                    .font_fallback
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            font_ligatures: Setting {
                value: defaults
                    .terminal
                    .font_ligatures
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            cursor_blink: Setting {
                value: defaults
                    .terminal
                    .cursor_blink
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            scroll_on_input: Setting {
                value: defaults
                    .terminal
                    .scroll_on_input
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            scroll_on_output: Setting {
                value: defaults
                    .terminal
                    .scroll_on_output
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            mac_option_as_meta: Setting {
                value: defaults
                    .terminal
                    .mac_option_as_meta
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            backspace_send: Setting {
                value: defaults
                    .terminal
                    .backspace_send
                    .clone()
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            delete_send: Setting {
                value: defaults
                    .terminal
                    .delete_send
                    .clone()
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            term_type: Setting {
                value: defaults.terminal.term_type.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            copy_on_select: Setting {
                value: defaults
                    .terminal
                    .copy_on_select
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            right_click_paste: Setting {
                value: defaults
                    .terminal
                    .right_click_paste
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            history_limit: Setting {
                value: defaults
                    .terminal
                    .history_limit
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            terminal_theme: Setting {
                value: defaults.terminal.theme.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            terminal_preset: Setting {
                value: defaults
                    .terminal
                    .color_preset
                    .ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            ui_appearance: Setting {
                value: defaults.ui.appearance.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            ui_language: Setting {
                value: defaults.ui.language.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            ui_accent: Setting {
                value: defaults.ui.accent.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
        };
        if let Some(value) = user.and_then(|patch| patch.ui.appearance) {
            result.ui_appearance = Setting {
                value,
                source: SettingsSource::User,
            };
        }
        if let Some(value) = user.and_then(|patch| patch.ui.language) {
            result.ui_language = Setting {
                value,
                source: SettingsSource::User,
            };
        }
        if let Some(value) = user.and_then(|patch| patch.ui.accent) {
            result.ui_accent = Setting {
                value,
                source: SettingsSource::User,
            };
        }
        if let Some(value) = user.and_then(|patch| patch.terminal.theme) {
            result.terminal_theme = Setting {
                value,
                source: SettingsSource::User,
            };
        }
        for (patch, source) in [
            (user, SettingsSource::User),
            (group, SettingsSource::Group),
            (host, SettingsSource::Host),
        ] {
            if let Some(patch) = patch {
                if source != SettingsSource::User
                    && (patch.ui != UiPatch::default() || patch.terminal.theme.is_some())
                {
                    return Err(SettingsError::Value);
                }
                patch.validate()?;
                result.apply(patch.clone(), source);
            }
        }
        Ok(result)
    }
    /// Shared schema for sparse documents; generation does not read user files.
    pub fn schema() -> schemars::Schema {
        schema_for!(SettingsPatch)
    }
}
impl ResolvedSettings {
    fn apply(&mut self, patch: SettingsPatch, source: SettingsSource) {
        if let Some(value) = patch.terminal.history_limit {
            self.history_limit = Setting { value, source };
        }
        if let Some(value) = patch.terminal.font_size {
            self.font_size = Setting { value, source };
        }
        if let Some(value) = patch.terminal.line_height_percent {
            self.line_height_percent = Setting { value, source };
        }
        if let Some(value) = patch.terminal.letter_spacing {
            self.letter_spacing = Setting { value, source };
        }
        if let Some(value) = patch.terminal.font_family {
            self.font_family = Setting { value, source };
        }
        if let Some(value) = patch.terminal.font_fallback {
            self.font_fallback = Setting { value, source };
        }
        if let Some(value) = patch.terminal.font_ligatures {
            self.font_ligatures = Setting { value, source };
        }
        if let Some(value) = patch.terminal.scroll_on_input {
            self.scroll_on_input = Setting { value, source };
        }
        if let Some(value) = patch.terminal.scroll_on_output {
            self.scroll_on_output = Setting { value, source };
        }
        if let Some(value) = patch.terminal.mac_option_as_meta {
            self.mac_option_as_meta = Setting { value, source };
        }
        if let Some(value) = patch.terminal.backspace_send {
            self.backspace_send = Setting { value, source };
        }
        if let Some(value) = patch.terminal.delete_send {
            self.delete_send = Setting { value, source };
        }
        if let Some(value) = patch.terminal.term_type {
            self.term_type = Setting { value, source };
        }
        if let Some(value) = patch.terminal.cursor_blink {
            self.cursor_blink = Setting { value, source };
        }
        if let Some(value) = patch.terminal.copy_on_select {
            self.copy_on_select = Setting { value, source };
        }
        if let Some(value) = patch.terminal.right_click_paste {
            self.right_click_paste = Setting { value, source };
        }
        if let Some(value) = patch.terminal.color_preset {
            self.terminal_preset = Setting { value, source };
        }
    }
}
#[cfg(test)]
#[path = "settings_tests.rs"]
mod settings_tests;
#[cfg(test)]
#[path = "settings_terminal_preset_tests.rs"]
mod terminal_preset_tests;
