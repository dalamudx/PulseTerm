use super::*;
use crate::{
    settings_edit::{TerminalSettingEdit, set_terminal_setting},
    settings_remove::{TerminalSettingKey, remove_terminal_setting},
};

type Result = std::result::Result<(), Box<dyn std::error::Error>>;
#[test]
fn terminal_preset_resolves_defaults_and_layer_precedence() -> Result {
    let defaults = SettingsResolver::resolve(None, None, None)?;
    assert_eq!(defaults.terminal_preset.value, TerminalPreset::Classic);
    assert_eq!(defaults.font_size.value, 14);
    let user = "[terminal]\ncolor_preset = 'midnight'\n";
    let group = "[terminal]\ncolor_preset = 'paper'\n";
    let host = "[terminal]\ncolor_preset = 'classic'\n";
    let selected = SettingsResolver::resolve(Some(user), Some(group), Some(host))?;
    assert_eq!(selected.terminal_preset.value, TerminalPreset::Classic);
    assert_eq!(selected.terminal_preset.source, SettingsSource::Host);
    assert!(SettingsPatch::parse("[terminal]\ncolor_preset = 'url(external)'\n").is_err());
    assert!(SettingsPatch::parse("[terminal]\ncolor_preset = 1\n").is_err());
    Ok(())
}
#[test]
fn terminal_preset_edit_and_reset_preserve_other_settings() -> Result {
    let original = "# keep\n[terminal]\ncolor_preset = 'paper' # palette\nfont_size = 14\n[ui]\nappearance = 'dark'\n";
    let changed = set_terminal_setting(
        original,
        TerminalSettingEdit::ColorPreset(TerminalPreset::Midnight),
    )?;
    assert!(changed.contains("# palette"));
    let resolved = SettingsResolver::resolve(Some(&changed), None, None)?;
    assert_eq!(resolved.terminal_preset.value, TerminalPreset::Midnight);
    assert_eq!(resolved.ui_appearance.value, UiAppearance::Dark);
    let removed = remove_terminal_setting(&changed, TerminalSettingKey::ColorPreset)?;
    assert!(!removed.contains("color_preset"));
    assert!(removed.contains("font_size = 14"));
    assert!(removed.contains("# keep"));
    assert_eq!(
        SettingsResolver::resolve(Some(&removed), None, None)?
            .terminal_preset
            .value,
        TerminalPreset::Classic
    );
    Ok(())
}
