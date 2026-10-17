//! Fixed terminal colors, shared by read-only previews and native actor initialization.
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Built-in base selection; remote OSC overrides remain independent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TerminalPreset {
    /// Existing default.
    #[default]
    Classic,
    /// Cool dark base.
    Midnight,
    /// Warm light base.
    Paper,
    /// One Dark terminal colors.
    OneDark,
    /// One Light terminal colors.
    OneLight,
    /// Catppuccin Mocha dark colors.
    CatppuccinMocha,
    /// Catppuccin Latte light colors.
    CatppuccinLatte,
    /// Dracula dark colors.
    Dracula,
    /// Solarized dark colors.
    SolarizedDark,
    /// Solarized light colors with base01 body text.
    SolarizedLight,
}

/// Bounded drawing metadata, never session-application authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct TerminalPresetColors {
    /// Sixteen ANSI colors in protocol order.
    pub ansi: [[u8; 3]; 16],
    /// Default foreground.
    pub foreground: [u8; 3],
    /// Default background.
    pub background: [u8; 3],
    /// Default cursor.
    pub cursor: [u8; 3],
}

/// A fixed preview from the same palette used by native startup.
#[derive(Serialize)]
pub struct TerminalPresetPaint {
    /// Supported preset.
    pub id: TerminalPreset,
    /// Drawing colors.
    pub colors: TerminalPresetColors,
}

impl TerminalPreset {
    /// Stable built-in catalogue; legacy identifiers retain their original colors.
    pub const ALL: [Self; 10] = [
        Self::Classic,
        Self::Midnight,
        Self::Paper,
        Self::OneDark,
        Self::OneLight,
        Self::CatppuccinMocha,
        Self::CatppuccinLatte,
        Self::Dracula,
        Self::SolarizedDark,
        Self::SolarizedLight,
    ];

    /// Stable settings value.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Classic => "classic",
            Self::Midnight => "midnight",
            Self::Paper => "paper",
            Self::OneDark => "one_dark",
            Self::OneLight => "one_light",
            Self::CatppuccinMocha => "catppuccin_mocha",
            Self::CatppuccinLatte => "catppuccin_latte",
            Self::Dracula => "dracula",
            Self::SolarizedDark => "solarized_dark",
            Self::SolarizedLight => "solarized_light",
        }
    }

    /// No user color strings or external paths are interpreted.
    pub fn colors(self) -> TerminalPresetColors {
        let (ansi, foreground, background) = match self {
            Self::Classic => (
                [
                    0x000000, 0xcd0000, 0x00cd00, 0xcdcd00, 0x0000ee, 0xcd00cd, 0x00cdcd, 0xe5e5e5,
                    0x7f7f7f, 0xff0000, 0x00ff00, 0xffff00, 0x5c5cff, 0xff00ff, 0x00ffff, 0xffffff,
                ],
                0xffffff,
                0x000000,
            ),
            Self::Midnight => (
                [
                    0x0f172a, 0xf87171, 0x4ade80, 0xfacc15, 0x60a5fa, 0xc084fc, 0x22d3ee, 0xcbd5e1,
                    0x64748b, 0xfca5a5, 0x86efac, 0xfde047, 0x93c5fd, 0xd8b4fe, 0x67e8f9, 0xf8fafc,
                ],
                0xe2e8f0,
                0x0f172a,
            ),
            Self::Paper => (
                [
                    0x1f2937, 0xb91c1c, 0x166534, 0xa16207, 0x1d4ed8, 0x7e22ce, 0x0e7490, 0x78716c,
                    0x57534e, 0x991b1b, 0x15803d, 0x854d0e, 0x1e40af, 0x6b21a8, 0x155e75, 0x44403c,
                ],
                0x1f2937,
                0xfafaf9,
            ),
            Self::OneDark => (
                [
                    0x282c34, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xabb2bf,
                    0x5c6370, 0xe06c75, 0x98c379, 0xe5c07b, 0x61afef, 0xc678dd, 0x56b6c2, 0xffffff,
                ],
                0xabb2bf,
                0x282c34,
            ),
            Self::OneLight => (
                [
                    0x383a42, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xa0a1a7,
                    0x4f525e, 0xe45649, 0x50a14f, 0xc18401, 0x4078f2, 0xa626a4, 0x0184bc, 0xffffff,
                ],
                0x383a42,
                0xfafafa,
            ),
            Self::CatppuccinMocha => (
                [
                    0x45475a, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xbac2de,
                    0x585b70, 0xf38ba8, 0xa6e3a1, 0xf9e2af, 0x89b4fa, 0xf5c2e7, 0x94e2d5, 0xa6adc8,
                ],
                0xcdd6f4,
                0x1e1e2e,
            ),
            Self::CatppuccinLatte => (
                [
                    0x5c5f77, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xacb0be,
                    0x6c6f85, 0xd20f39, 0x40a02b, 0xdf8e1d, 0x1e66f5, 0xea76cb, 0x179299, 0xbcc0cc,
                ],
                0x4c4f69,
                0xeff1f5,
            ),
            Self::Dracula => (
                [
                    0x21222c, 0xff5555, 0x50fa7b, 0xf1fa8c, 0xbd93f9, 0xff79c6, 0x8be9fd, 0xf8f8f2,
                    0x6272a4, 0xff6e6e, 0x69ff94, 0xffffa5, 0xd6acff, 0xff92df, 0xa4ffff, 0xffffff,
                ],
                0xf8f8f2,
                0x282a36,
            ),
            Self::SolarizedDark => (
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
                0x839496,
                0x002b36,
            ),
            Self::SolarizedLight => (
                [
                    0x073642, 0xdc322f, 0x859900, 0xb58900, 0x268bd2, 0xd33682, 0x2aa198, 0xeee8d5,
                    0x002b36, 0xcb4b16, 0x586e75, 0x657b83, 0x839496, 0x6c71c4, 0x93a1a1, 0xfdf6e3,
                ],
                0x586e75,
                0xfdf6e3,
            ),
        };
        TerminalPresetColors {
            ansi: ansi.map(rgb),
            foreground: rgb(foreground),
            background: rgb(background),
            cursor: rgb(foreground),
        }
    }

    /// All supported previews in stable display order.
    pub fn previews() -> [TerminalPresetPaint; Self::ALL.len()] {
        Self::ALL.map(|id| TerminalPresetPaint {
            id,
            colors: id.colors(),
        })
    }
}

fn rgb(value: u32) -> [u8; 3] {
    [(value >> 16) as u8, (value >> 8) as u8, value as u8]
}

#[cfg(test)]
mod tests {
    use super::TerminalPreset;
    use crate::{
        settings::{SettingsResolver, SettingsSource},
        settings_edit::{TerminalSettingEdit, set_terminal_setting},
        settings_remove::{TerminalSettingKey, remove_terminal_setting},
    };
    type TestResult = Result<(), Box<dyn std::error::Error>>;

    #[test]
    fn builtin_catalog_preserves_siblings_and_scope_inheritance() -> TestResult {
        let original =
            "# keep\n[terminal]\nfont_size = 19 # font\ncolor_preset = 'classic' # palette\n";
        for preset in TerminalPreset::ALL {
            let written = set_terminal_setting(original, TerminalSettingEdit::ColorPreset(preset))?;
            assert!(written.contains("# palette"));
            assert!(written.contains("# font"));
            let saved = SettingsResolver::resolve(Some(&written), None, None)?;
            assert_eq!(saved.terminal_preset.value, preset);
            assert_eq!(saved.font_size.value, 19);
            assert_eq!(saved.terminal_preset.source, SettingsSource::User);
            let group = format!("[terminal]\ncolor_preset = '{}'", preset.as_str());
            let inherited = SettingsResolver::resolve(Some(&written), Some(&group), None)?;
            assert_eq!(inherited.terminal_preset.source, SettingsSource::Group);
            assert_eq!(inherited.terminal_preset.value, preset);
            let host = SettingsResolver::resolve(
                Some(&written),
                Some(&group),
                Some("terminal.color_preset = 'classic'"),
            )?;
            assert_eq!(host.terminal_preset.source, SettingsSource::Host);
            assert_eq!(host.terminal_preset.value, TerminalPreset::Classic);
            let reset = remove_terminal_setting(&written, TerminalSettingKey::ColorPreset)?;
            assert_eq!(
                SettingsResolver::resolve(Some(&reset), None, None)?
                    .terminal_preset
                    .value,
                TerminalPreset::Classic
            );
        }
        // Public bounded drawing data doubles as the development component fixture.
        println!(
            "BUILTIN_PAINT={}",
            serde_json::to_string(&TerminalPreset::previews())?
        );
        Ok(())
    }

    #[test]
    fn builtin_catalog_body_text_meets_contrast_floor() {
        fn luminance(rgb: [u8; 3]) -> f64 {
            let c = rgb.map(|v| {
                let x = f64::from(v) / 255.0;
                if x <= 0.04045 {
                    x / 12.92
                } else {
                    ((x + 0.055) / 1.055).powf(2.4)
                }
            });
            c[0] * 0.2126 + c[1] * 0.7152 + c[2] * 0.0722
        }
        for preset in TerminalPreset::ALL {
            let colors = preset.colors();
            let a = luminance(colors.foreground);
            let b = luminance(colors.background);
            assert!((a.max(b) + 0.05) / (a.min(b) + 0.05) >= 4.5, "{preset:?}");
        }
    }
}
