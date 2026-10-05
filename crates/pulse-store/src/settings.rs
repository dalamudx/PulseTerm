//! Settings prototypes for three terminal display fields, never executable policy.
use regex::Regex;
use schemars::{JsonSchema, schema_for};
use serde::{Deserialize, Serialize};
use std::sync::LazyLock;
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
}
/// Every field is optional, independently resolved and independently validated.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct TerminalPatch {
    /// Initial supported font-size range, in CSS pixels, inclusive.
    #[schemars(range(min = 6, max = 72))]
    pub font_size: Option<u16>,
    /// A single font name, at most 128 Unicode scalars, using safe characters.
    #[schemars(
        length(min = 1, max = 128),
        regex(pattern = r"^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$")
    )]
    pub font_family: Option<String>,
    /// Explicit false still replaces lower-priority true.
    pub cursor_blink: Option<bool>,
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
        if let Some(name) = &self.terminal.font_family {
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
    /// Effective safe single font family.
    pub font_family: Setting<String>,
    /// Effective cursor blink policy.
    pub cursor_blink: Setting<bool>,
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
        let defaults = SettingsPatch::parse(DEFAULTS_TOML)?;
        let mut result = ResolvedSettings {
            font_size: Setting {
                value: defaults.terminal.font_size.ok_or(SettingsError::Defaults)?,
                source: SettingsSource::Default,
            },
            font_family: Setting {
                value: defaults
                    .terminal
                    .font_family
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
        };
        for (text, source) in [
            (user, SettingsSource::User),
            (group, SettingsSource::Group),
            (host, SettingsSource::Host),
        ] {
            if let Some(text) = text {
                result.apply(SettingsPatch::parse(text)?, source);
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
        if let Some(value) = patch.terminal.font_size {
            self.font_size = Setting { value, source };
        }
        if let Some(value) = patch.terminal.font_family {
            self.font_family = Setting { value, source };
        }
        if let Some(value) = patch.terminal.cursor_blink {
            self.cursor_blink = Setting { value, source };
        }
    }
}
#[cfg(test)]
#[path = "settings_tests.rs"]
mod settings_tests;
