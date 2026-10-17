//! User-level terminal palette policy, frozen by native startup.
use super::TerminalPreset;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Fixed palettes retain the existing color_preset selection.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalThemeMode {
    /// Use the fixed palette, including the existing default.
    #[default]
    Fixed,
    /// Resolve the native window's system appearance at session startup.
    System,
}
/// One atomic policy; partially supplied or unknown fields are rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TerminalTheme {
    /// Palette selection policy.
    pub mode: TerminalThemeMode,
    /// Palette for a light or unavailable native system appearance.
    pub light: TerminalPreset,
    /// Palette for a dark native system appearance.
    pub dark: TerminalPreset,
}
impl Default for TerminalTheme {
    fn default() -> Self {
        Self {
            mode: TerminalThemeMode::Fixed,
            light: TerminalPreset::Paper,
            dark: TerminalPreset::Midnight,
        }
    }
}
impl TerminalTheme {
    /// Pure selection using a backend-owned appearance snapshot.
    pub fn preset(self, fixed: TerminalPreset, system_dark: bool) -> TerminalPreset {
        match self.mode {
            TerminalThemeMode::Fixed => fixed,
            TerminalThemeMode::System if system_dark => self.dark,
            TerminalThemeMode::System => self.light,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::{SettingsResolver, SettingsSource};
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    use crate::settings_remove::{TerminalSettingKey, remove_terminal_setting};
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn system_theme_roundtrip_reset_and_invalid_documents() -> TestResult {
        let theme = TerminalTheme {
            mode: TerminalThemeMode::System,
            light: TerminalPreset::CatppuccinLatte,
            dark: TerminalPreset::Dracula,
        };
        for text in [
            "# retained\n[terminal]\ncolor_preset='classic' # fixed\n",
            "terminal.color_preset='classic' # fixed\n",
            "terminal={color_preset='classic'} # fixed\n",
            "[terminal]\ncolor_preset='classic' # fixed\n[terminal.theme] # policy\nmode='fixed' # branch\nlight='paper'\ndark='midnight'\n",
            "terminal.color_preset='classic' # fixed\nterminal.theme.mode='fixed' # branch\nterminal.theme.light='paper'\nterminal.theme.dark='midnight'\n",
            "[terminal]\ncolor_preset='classic' # fixed\ntheme.mode='fixed' # branch\ntheme.light='paper'\ntheme.dark='midnight'\n",
        ] {
            let written = set_terminal_setting(text, TerminalSettingEdit::Theme(theme))?;
            assert!(written.contains("# fixed"));
            let resolved = SettingsResolver::resolve(Some(&written), None, None)?;
            assert_eq!(resolved.terminal_theme.value, theme);
            assert_eq!(resolved.terminal_theme.source, SettingsSource::User);
            let overwritten = set_terminal_setting(
                &written,
                TerminalSettingEdit::Theme(TerminalTheme::default()),
            )?;
            assert_eq!(
                SettingsResolver::resolve(Some(&overwritten), None, None)?
                    .terminal_theme
                    .value
                    .mode,
                TerminalThemeMode::Fixed
            );
            let reset = remove_terminal_setting(&written, TerminalSettingKey::Theme)?;
            assert_eq!(
                SettingsResolver::resolve(Some(&reset), None, None)?
                    .terminal_theme
                    .source,
                SettingsSource::Default
            );
            assert!(reset.contains("# fixed"));
            for comment in ["# policy", "# branch"] {
                if text.contains(comment) {
                    assert!(written.contains(comment));
                    assert!(reset.contains(comment));
                }
            }
        }
        for invalid in [
            "{mode='system',light='paper'}",
            "{mode='auto',light='paper',dark='midnight'}",
            "{mode='system',light='paper',dark='midnight',css='red'}",
        ] {
            assert!(
                SettingsResolver::resolve(Some(&format!("terminal.theme={invalid}")), None, None)
                    .is_err()
            );
        }
        Ok(())
    }
    #[test]
    fn system_theme_freezes_branch_and_preserves_scope_overrides() -> TestResult {
        let user = "[terminal]\ncolor_preset='classic'\ntheme={mode='system',light='catppuccin_latte',dark='dracula'}\n";
        let resolved = SettingsResolver::resolve(Some(user), None, None)?;
        let frozen = resolved.terminal_startup(true);
        assert_eq!(frozen.preset, TerminalPreset::Dracula);
        assert_eq!(
            resolved.terminal_startup(false).preset,
            TerminalPreset::CatppuccinLatte
        );
        assert_eq!(frozen.preset, TerminalPreset::Dracula);
        for (group, host, expected) in [
            (
                Some("terminal.color_preset='paper'"),
                None,
                TerminalPreset::Paper,
            ),
            (
                Some("terminal.color_preset='paper'"),
                Some("terminal.color_preset='one_dark'"),
                TerminalPreset::OneDark,
            ),
        ] {
            let scoped = SettingsResolver::resolve(Some(user), group, host)?;
            assert_eq!(scoped.terminal_startup(true).preset, expected);
            assert_eq!(scoped.terminal_startup(false).preset, expected);
        }
        assert!(SettingsResolver::resolve(None, Some(user), None).is_err());
        assert!(SettingsResolver::resolve(None, None, Some(user)).is_err());
        let fixed = SettingsResolver::resolve(Some("terminal.color_preset='dracula'"), None, None)?;
        assert_eq!(
            fixed.terminal_startup(false).preset,
            TerminalPreset::Dracula
        );
        assert_eq!(fixed.terminal_startup(true).preset, TerminalPreset::Dracula);
        Ok(())
    }
}
