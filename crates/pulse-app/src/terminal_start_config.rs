//! Frozen native terminal startup configuration.
/// Backend-selected base colors and retained history; never execution authority.
#[derive(Clone)]
pub struct TerminalStartConfig {
    /// Base palette installed before output parsing.
    pub palette: pulse_term::QueryPalette,
    /// Maximum retained primary history rows across hot and cold tiers.
    pub history_limit: usize,
    /// Frozen TERM advertisement, independent of parser emulation.
    pub term_type: pulse_store::settings::TerminalType,
    /// Frozen terminal-view behavior, independent of input admission.
    pub behavior: pulse_store::profile_editor::ProfileAppearance,
}
impl From<pulse_term::QueryPalette> for TerminalStartConfig {
    fn from(palette: pulse_term::QueryPalette) -> Self {
        Self {
            palette,
            history_limit: 100_000,
            term_type: pulse_store::settings::TerminalType::default(),
            behavior: Default::default(),
        }
    }
}
impl From<pulse_store::settings::TerminalStartup> for TerminalStartConfig {
    fn from(startup: pulse_store::settings::TerminalStartup) -> Self {
        Self {
            palette: crate::terminal_palette::query_palette_from_preset(startup.preset),
            history_limit: startup.history_limit,
            term_type: startup.term_type,
            behavior: pulse_store::profile_editor::ProfileAppearance {
                mac_option_is_meta: Some(startup.mac_option_as_meta),
                backspace_send: Some(startup.backspace_send.into()),
                delete_send: Some(startup.delete_send.into()),
                ..Default::default()
            },
        }
    }
}
impl TerminalStartConfig {
    /// Replace only the prepared backend default; later explicit Profile TERM entries still win.
    pub(crate) fn apply_local_term(&self, command: &mut pulse_pty::PtyCommand) {
        if let Some((_, value)) = command.env.iter_mut().find(|(key, _)| key == "TERM")
            && value == "xterm-256color"
        {
            *value = self.term_type.as_str().into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn terminal_behavior_term_changes_default_but_not_explicit_profile() {
        use pulse_store::settings::TerminalType;
        let mut config = TerminalStartConfig::from(pulse_term::QueryPalette::default());
        config.term_type = TerminalType::Vt100;
        let mut command = pulse_pty::PtyCommand {
            program: "sh".into(),
            args: vec![],
            cwd: None,
            env: vec![
                ("TERM".into(), "xterm-256color".into()),
                ("TERM".into(), "screen-256color".into()),
            ],
            env_remove: vec![],
        };
        config.apply_local_term(&mut command);
        assert_eq!(command.env[0].1, "vt100");
        assert_eq!(command.env[1].1, "screen-256color");
        command.env = vec![("TERM".into(), "ansi".into())];
        config.apply_local_term(&mut command);
        assert_eq!(command.env[0].1, "ansi");
    }
    #[test]
    fn organized_behavior_freezes_key_preferences_for_registered_session()
    -> Result<(), pulse_store::settings::SettingsError> {
        let settings = pulse_store::settings::SettingsResolver::resolve(
            Some(
                "[terminal]\ncolor_preset=\"paper\"\nmac_option_as_meta=true\nbackspace_send=\"bs\"\ndelete_send=\"del\"\nterm_type=\"vt100\"\n",
            ),
            None,
            None,
        )?;
        let config = TerminalStartConfig::from(settings.terminal_startup(false));
        assert_eq!(config.behavior.mac_option_is_meta, Some(true));
        assert_eq!(config.behavior.backspace_send.as_deref(), Some("bs"));
        assert_eq!(config.behavior.delete_send.as_deref(), Some("del"));
        assert_eq!(config.term_type.as_str(), "vt100");
        assert_eq!(
            config.palette,
            crate::terminal_palette::query_palette_from_preset(
                pulse_store::settings::TerminalPreset::Paper
            )
        );
        Ok(())
    }
}
