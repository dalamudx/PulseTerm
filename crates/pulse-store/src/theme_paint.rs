//! Fixed paint data, not imported text, CSS, a file format, or view/application authority.
use super::{NeutralScale, ThemeAppearance, ThemeDocument, ThemeError};
use serde::Serialize;
/// Maximum compact encoded projection; no names, arbitrary property names or original configuration.
pub const MAX_THEME_PAINT_BYTES: usize = 4096;
macro_rules! slots {
 ($($variant:ident=>$variable:literal),+$(,)?)=>{
 /// Closed semantic slots. Imported CSS property names never cross this boundary.
 #[derive(Clone,Copy,Debug,PartialEq,Eq,Serialize)]
 #[serde(rename_all="camelCase")]
 pub enum UiColorSlot {$(#[doc=concat!("Fixed ",$variable," semantic color.")] $variant),+}
 impl UiColorSlot {
 fn from_variable(value:&str)->Result<Self,ThemeError>{match value{$($variable=>Ok(Self::$variant),)+_=>Err(ThemeError::Document)}}
 /// Fixed local CSS variable identifier. Color assignment still requires a current view/CSP guard.
 pub fn css_variable(self)->&'static str {match self{$(Self::$variant=>$variable),+}}
 }
 };
}
slots! {
 BgBase=>"--bg-base",BgSubtle=>"--bg-subtle",BgSurface=>"--bg-surface",BgHover=>"--bg-hover",BgActive=>"--bg-active",
 BorderSubtle=>"--border-subtle",Border=>"--border",BorderStrong=>"--border-strong",FgMuted=>"--fg-muted",Fg=>"--fg",
 AccentBg=>"--accent-bg",AccentBgHover=>"--accent-bg-hover",Accent=>"--accent",AccentHover=>"--accent-hover",AccentContrast=>"--accent-contrast",AccentFg=>"--accent-fg",
 DangerBg=>"--danger-bg",Danger=>"--danger",DangerFg=>"--danger-fg",WarningBg=>"--warning-bg",Warning=>"--warning",WarningFg=>"--warning-fg",
 SuccessBg=>"--success-bg",Success=>"--success",SuccessFg=>"--success-fg",InfoBg=>"--info-bg",Info=>"--info",InfoFg=>"--info-fg"
}
/// Opaque RGB8 paint value; no alpha, expressions or inferred default color.
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct UiPaintColor {
    slot: UiColorSlot,
    color: [u8; 3],
}
impl UiPaintColor {
    /// Whitelisted semantic target.
    pub fn slot(&self) -> UiColorSlot {
        self.slot
    }
    /// Opaque red/green/blue channels.
    pub fn color(&self) -> [u8; 3] {
        self.color
    }
}
/// Terminal colors independent from UI scale selection. All sixteen ANSI slots are mandatory.
#[derive(Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalPaint {
    foreground: [u8; 3],
    background: [u8; 3],
    ansi: [[u8; 3]; 16],
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    cursor_text: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_background: Option<[u8; 3]>,
    #[serde(skip_serializing_if = "Option::is_none")]
    selection_foreground: Option<[u8; 3]>,
}
impl TerminalPaint {
    /// Foreground RGB8.
    pub fn foreground(&self) -> [u8; 3] {
        self.foreground
    }
    /// Background RGB8.
    pub fn background(&self) -> [u8; 3] {
        self.background
    }
    /// ANSI order, including magenta at 5 and bright magenta at 13.
    pub fn ansi(&self) -> &[[u8; 3]; 16] {
        &self.ansi
    }
    /// Cursor background; absence is not filled with an inferred color.
    pub fn cursor(&self) -> Option<[u8; 3]> {
        self.cursor
    }
    /// Cursor foreground.
    pub fn cursor_text(&self) -> Option<[u8; 3]> {
        self.cursor_text
    }
    /// Selection background.
    pub fn selection_background(&self) -> Option<[u8; 3]> {
        self.selection_background
    }
    /// Selection foreground.
    pub fn selection_foreground(&self) -> Option<[u8; 3]> {
        self.selection_foreground
    }
}
/// Immutable, serialize-only paint image; does not carry authorization or claim that CSS was applied.
/// No external deserialization is supported:
/// ```compile_fail
/// let _:pulse_store::theme::ThemePaintData=serde_json::from_str("{}").unwrap();
/// ```
#[derive(Clone, PartialEq, Eq, Serialize)]
pub struct ThemePaintData {
    version: u8,
    appearance: ThemeAppearance,
    neutral: NeutralScale,
    accent: String,
    terminal: TerminalPaint,
    overrides: Vec<UiPaintColor>,
    #[serde(rename = "lowForegroundContrast")]
    low_foreground_contrast: bool,
}
impl ThemePaintData {
    /// Project a validated document without labels, file data, arbitrary CSS keys or implicit defaults.
    /// Accent is a validated selector/seed, not generated Radix CSS. Future unknown slots fail closed.
    pub fn from_document(document: &ThemeDocument) -> Result<Self, ThemeError> {
        let t = document.terminal();
        let overrides = document
            .ui()
            .overrides()
            .iter()
            .map(|(key, color)| {
                Ok(UiPaintColor {
                    slot: UiColorSlot::from_variable(key)?,
                    color: color.channels(),
                })
            })
            .collect::<Result<Vec<_>, ThemeError>>()?;
        Ok(Self {
            version: 1,
            appearance: document.appearance(),
            neutral: document.ui().neutral(),
            accent: document.ui().accent().value().into(),
            terminal: TerminalPaint {
                foreground: t.foreground.channels(),
                background: t.background.channels(),
                ansi: t.ansi().map(|c| c.channels()),
                cursor: t.cursor.map(|c| c.channels()),
                cursor_text: t.cursor_text.map(|c| c.channels()),
                selection_background: t.selection_background.map(|c| c.channels()),
                selection_foreground: t.selection_foreground.map(|c| c.channels()),
            },
            overrides,
            low_foreground_contrast: t.contrast().warning.is_some(),
        })
    }
    /// Explicit document appearance, not a system-theme observer.
    pub fn appearance(&self) -> ThemeAppearance {
        self.appearance
    }
    /// Known neutral family selector; colors still need a trusted local scale resolver.
    pub fn neutral(&self) -> NeutralScale {
        self.neutral
    }
    /// Known accent family or canonical RGB8 seed. Never interpolate as CSS text.
    pub fn accent(&self) -> &str {
        &self.accent
    }
    /// Complete fixed terminal colors.
    pub fn terminal(&self) -> &TerminalPaint {
        &self.terminal
    }
    /// At most twenty-eight closed semantic slots; never user property identifiers.
    pub fn overrides(&self) -> &[UiPaintColor] {
        &self.overrides
    }
    /// Existing advisory WCAG 4.5:1 check for terminal text only. UI scales are not resolved here.
    pub fn low_foreground_contrast(&self) -> bool {
        self.low_foreground_contrast
    }
    /// Compact bounded JSON image, not a command/stylesheet or permission receipt.
    pub fn to_json(&self) -> Result<String, ThemeError> {
        let encoded = serde_json::to_string(self).map_err(|_| ThemeError::Encoding)?;
        if encoded.len() > MAX_THEME_PAINT_BYTES {
            return Err(ThemeError::Budget);
        }
        Ok(encoded)
    }
}
#[cfg(test)]
#[path = "theme_paint_tests.rs"]
mod tests;
