//! Window UI preferences; no terminal palette, credential or execution authority.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
/// Persisted UI mode. System follows this window's media query at display time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiAppearance {
    /// Use the operating system's preferred color scheme.
    System,
    /// Use the existing light interface.
    Light,
    /// Use the dark interface.
    Dark,
}
impl UiAppearance {
    /// Exact scalar written to the supported user setting.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::Light => "light",
            Self::Dark => "dark",
        }
    }
}
/// A fixed Radix accent family; never user-supplied CSS.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum UiAccent {
    /// Cyan accent.
    Cyan,
    /// Blue accent.
    Blue,
    /// Green accent.
    Green,
    /// Amber accent.
    Amber,
    /// Red accent.
    Red,
}
impl UiAccent {
    /// Exact scalar written to the user document.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Cyan => "cyan",
            Self::Blue => "blue",
            Self::Green => "green",
            Self::Amber => "amber",
            Self::Red => "red",
        }
    }
}
/// Supported interface languages; no arbitrary locale or resource path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
pub enum UiLanguage {
    /// Simplified Chinese.
    #[serde(rename = "zh-CN")]
    Chinese,
    /// English.
    #[serde(rename = "en")]
    English,
}
impl UiLanguage {
    /// Exact supported user preference.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Chinese => "zh-CN",
            Self::English => "en",
        }
    }
}
/// Sparse user-level UI preferences, separate from terminal overlays.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(default, deny_unknown_fields)]
pub struct UiPatch {
    /// An omitted preference inherits the compiled light default.
    pub appearance: Option<UiAppearance>,
    /// Fixed Radix accent; omission inherits the compiled cyan default.
    pub accent: Option<UiAccent>,
    /// Omission inherits the compiled Chinese interface.
    pub language: Option<UiLanguage>,
}

#[cfg(test)]
mod tests {
    use crate::{
        settings::{SettingsResolver, SettingsSource, UiAccent},
        settings_edit::{TerminalSettingEdit, set_terminal_setting},
        settings_remove::{TerminalSettingKey, remove_terminal_setting},
    };
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn ui_accent_roundtrip_preserves_appearance_terminal_and_comments() -> TestResult {
        for text in [
            "# preserve\n[ui]\nappearance='dark'\naccent='cyan' # chosen\n[terminal]\nfont_size=19\n",
            "ui.appearance='dark'\nui.accent='cyan' # chosen\nterminal.font_size=19\n",
            "ui={appearance='dark', accent='cyan'} # chosen\n[terminal]\nfont_size=19\n",
        ] {
            let before = SettingsResolver::resolve(Some(text), None, None)?;
            for accent in [
                UiAccent::Cyan,
                UiAccent::Blue,
                UiAccent::Green,
                UiAccent::Amber,
                UiAccent::Red,
            ] {
                let written = set_terminal_setting(text, TerminalSettingEdit::UiAccent(accent))?;
                let saved = SettingsResolver::resolve(Some(&written), None, None)?;
                assert_eq!(saved.ui_accent.value, accent);
                assert_eq!(saved.ui_accent.source, SettingsSource::User);
                assert_eq!(saved.ui_appearance, before.ui_appearance);
                assert_eq!(saved.font_size, before.font_size);
                assert_eq!(saved.terminal_preset, before.terminal_preset);
                assert!(written.contains("# chosen"));
                let reset = remove_terminal_setting(&written, TerminalSettingKey::UiAccent)?;
                let inherited = SettingsResolver::resolve(Some(&reset), None, None)?;
                assert_eq!(inherited.ui_accent.value, UiAccent::Cyan);
                assert_eq!(inherited.ui_accent.source, SettingsSource::Default);
                assert_eq!(inherited.ui_appearance, before.ui_appearance);
                assert_eq!(inherited.font_size, before.font_size);
                assert!(reset.contains("# chosen"));
            }
        }
        Ok(())
    }
    #[test]
    fn ui_accent_rejects_unknown_values_and_non_user_overlays() -> TestResult {
        for text in [
            "ui.accent='purple'",
            "ui.accent='var(--red-9)'",
            "ui.accent=1",
            "ui.accent=true",
        ] {
            assert!(SettingsResolver::resolve(Some(text), None, None).is_err());
        }
        for overlays in [
            (None, Some("ui.accent='blue'"), None),
            (None, None, Some("ui.accent='blue'")),
        ] {
            assert!(SettingsResolver::resolve(overlays.0, overlays.1, overlays.2).is_err());
        }
        let missing = SettingsResolver::resolve(Some("ui.appearance='dark'"), None, None)?;
        assert_eq!(missing.ui_accent.source, SettingsSource::Default);
        assert_eq!(missing.ui_accent.value, UiAccent::Cyan);
        Ok(())
    }
    #[test]
    fn language_roundtrip_preserves_other_preferences_and_comments() -> TestResult {
        use crate::settings::UiLanguage;
        for text in [
            "# retain\n[ui]\nappearance='dark'\naccent='blue'\nlanguage='zh-CN' # choice\n[terminal]\nfont_size=19\n",
            "ui={appearance='dark', accent='blue', language='zh-CN'} # choice\nterminal.font_size=19\n",
            "ui.appearance='dark'\nui.accent='blue'\nui.language='zh-CN' # choice\nterminal.font_size=19\n",
        ] {
            let before = SettingsResolver::resolve(Some(text), None, None)?;
            let saved =
                set_terminal_setting(text, TerminalSettingEdit::UiLanguage(UiLanguage::English))?;
            let resolved = SettingsResolver::resolve(Some(&saved), None, None)?;
            assert_eq!(resolved.ui_language.value, UiLanguage::English);
            assert_eq!(resolved.ui_language.source, SettingsSource::User);
            assert_eq!(resolved.ui_accent, before.ui_accent);
            assert_eq!(resolved.ui_appearance, before.ui_appearance);
            assert_eq!(resolved.font_size, before.font_size);
            assert!(saved.contains("# choice"));
            let reset = remove_terminal_setting(&saved, TerminalSettingKey::UiLanguage)?;
            let inherited = SettingsResolver::resolve(Some(&reset), None, None)?;
            assert_eq!(inherited.ui_language.value, UiLanguage::Chinese);
            assert_eq!(inherited.ui_language.source, SettingsSource::Default);
            assert_eq!(inherited.font_size, before.font_size);
            assert!(reset.contains("# choice"));
        }
        Ok(())
    }
    #[test]
    fn language_rejects_unsupported_locales_and_non_user_overlays() -> TestResult {
        for text in [
            "ui.language='en-US'",
            "ui.language='../../en'",
            "ui.language='EN'",
            "ui.language=1",
            "ui.language=true",
        ] {
            assert!(SettingsResolver::resolve(Some(text), None, None).is_err());
        }
        assert!(SettingsResolver::resolve(None, Some("ui.language='en'"), None).is_err());
        assert!(SettingsResolver::resolve(None, None, Some("ui.language='en'")).is_err());
        Ok(())
    }
}
