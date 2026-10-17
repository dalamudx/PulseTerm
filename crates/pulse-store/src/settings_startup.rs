//! Native-only preferences frozen together from one settings snapshot.
use super::{ResolvedSettings, TerminalPreset};
/// Initial retained history and base colors; not execution authority.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TerminalStartup {
    /// Backend-selected base color preset.
    pub preset: TerminalPreset,
    /// Maximum retained hot and cold history rows.
    pub history_limit: usize,
    /// Capability name advertised at session startup.
    pub term_type: super::TerminalType,
    /// Frozen Option/Meta behavior for the terminal view.
    pub mac_option_as_meta: bool,
    /// Frozen Backspace byte choice.
    pub backspace_send: &'static str,
    /// Frozen Delete byte choice.
    pub delete_send: &'static str,
}
impl From<TerminalPreset> for TerminalStartup {
    fn from(preset: TerminalPreset) -> Self {
        Self {
            preset,
            history_limit: 100_000,
            term_type: super::TerminalType::default(),
            mac_option_as_meta: false,
            backspace_send: "del",
            delete_send: "csi",
        }
    }
}
impl ResolvedSettings {
    /// Freeze both preferences from this same resolved settings snapshot.
    pub fn terminal_startup(&self, system_dark: bool) -> TerminalStartup {
        TerminalStartup {
            preset: if matches!(
                self.terminal_preset.source,
                super::SettingsSource::Group | super::SettingsSource::Host
            ) {
                self.terminal_preset.value
            } else {
                self.terminal_theme
                    .value
                    .preset(self.terminal_preset.value, system_dark)
            },
            history_limit: self.history_limit.value as usize,
            term_type: self.term_type.value,
            mac_option_as_meta: self.mac_option_as_meta.value,
            backspace_send: if self.backspace_send.value == "bs" {
                "bs"
            } else {
                "del"
            },
            delete_send: if self.delete_send.value == "del" {
                "del"
            } else {
                "csi"
            },
        }
    }
}
