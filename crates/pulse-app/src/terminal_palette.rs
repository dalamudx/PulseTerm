//! Terminal base is independent of UI tokens and remote OSC overrides.
/// Fixed native selection; preview data and protocol replies use the same RGB source.
pub fn query_palette_from_preset(
    preset: pulse_store::settings::TerminalPreset,
) -> pulse_term::QueryPalette {
    let colors = preset.colors();
    pulse_term::QueryPalette {
        ansi: colors.ansi,
        foreground: colors.foreground,
        background: colors.background,
        cursor: colors.cursor,
    }
}
use pulse_store::theme::ThemeDocument;
use pulse_term::QueryPalette;

/// Construct exactly the validated terminal ANSI/default colors. Missing cursor
/// explicitly inherits foreground, matching xterm's base cursor semantics.
/// Selection and cursor text are not protocol query colors and are not guessed.
pub fn query_palette_from_theme(document: &ThemeDocument) -> QueryPalette {
    let terminal = document.terminal();
    QueryPalette {
        ansi: terminal.ansi().map(|color| color.channels()),
        foreground: terminal.foreground.channels(),
        background: terminal.background.channels(),
        cursor: terminal.cursor.unwrap_or(terminal.foreground).channels(),
    }
}

#[cfg(test)]
mod preset_tests {
    #[test]
    fn terminal_preset_classic_preserves_the_existing_base() {
        use pulse_store::settings::TerminalPreset;
        assert_eq!(
            super::query_palette_from_preset(TerminalPreset::Classic),
            pulse_term::QueryPalette::default()
        );
        for preset in TerminalPreset::ALL {
            let palette = super::query_palette_from_preset(preset);
            let colors = preset.colors();
            assert_eq!(palette.ansi, colors.ansi);
            assert_eq!(palette.foreground, colors.foreground);
            assert_eq!(palette.background, colors.background);
        }
    }
}
