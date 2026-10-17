use super::{
    ContrastReport, Rgb8, TerminalScheme, ThemeError, VsCodeTheme, budget, import_alacritty,
    import_ghostty, import_itermcolors, import_vscode, import_windows_terminal, validate_name,
};
use std::collections::BTreeMap;
/// Caller explicitly selects the importer; content and filename guessing are forbidden.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeImportFormat {
    /// Complete Windows Terminal named JSON scheme.
    WindowsTerminal,
    /// Alacritty color-only TOML with an explicit caller name.
    Alacritty,
    /// Ghostty color-only directives with an explicit caller name.
    Ghostty,
    /// Named strict VS Code JSON, terminal plus fixed UI overrides.
    VsCode,
    /// Explicit-sRGB iTerm XML; components quantized to RGB8.
    ItermXml,
    /// Current or legacy Base16 YAML, explicit system and embedded name.
    Base16Yaml,
    /// Current or legacy Base24 YAML, retains original 24 source colors.
    Base24Yaml,
}
/// Safe conversion metadata; no imported names, colors, paths, or parser errors.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThemeImportNotes {
    /// Fixed UI variables retained by the conversion.
    pub ui_override_count: usize,
    /// Unmapped nonterminal editor colors, never silently applied.
    pub ignored_editor_color_count: usize,
    /// Import uses nearest-RGB8 quantization of sRGB float components.
    pub rgb8_quantization: bool,
    /// Base palette projects to ANSI16; extended/source colors remain in base_scheme().
    pub base_palette_projection: bool,
}
enum Content {
    Terminal(TerminalScheme),
    Combined(VsCodeTheme),
    Base(super::BaseScheme),
}
/// Validated draft fragment, never a complete application theme or file write request.
/// Private content prevents metadata substitution; no Debug or Serialize for imported text.
pub struct ThemeImport {
    format: ThemeImportFormat,
    content: Content,
}
impl ThemeImport {
    /// Explicit parser used for this draft.
    pub fn format(&self) -> ThemeImportFormat {
        self.format
    }
    /// Complete validated ANSI16 terminal palette and display name.
    pub fn terminal(&self) -> &TerminalScheme {
        match &self.content {
            Content::Terminal(s) => s,
            Content::Combined(s) => s.terminal(),
            Content::Base(s) => s.terminal(),
        }
    }
    /// None for terminal-only formats; Some(empty) for VS Code with no mapped UI colors.
    pub fn ui_overrides(&self) -> Option<&BTreeMap<&'static str, Rgb8>> {
        match &self.content {
            Content::Terminal(_) | Content::Base(_) => None,
            Content::Combined(s) => Some(s.ui_overrides()),
        }
    }
    /// Original complete Base16/24 source colors; never silently dropped by ANSI16 projection.
    pub fn base_scheme(&self) -> Option<&super::BaseScheme> {
        match &self.content {
            Content::Base(s) => Some(s),
            _ => None,
        }
    }
    /// Conversion coverage and quantization metadata, without source text.
    pub fn notes(&self) -> ThemeImportNotes {
        ThemeImportNotes {
            ui_override_count: self.ui_overrides().map_or(0, BTreeMap::len),
            ignored_editor_color_count: match &self.content {
                Content::Combined(s) => s.ignored_color_count(),
                _ => 0,
            },
            rgb8_quantization: self.format == ThemeImportFormat::ItermXml,
            base_palette_projection: matches!(self.content, Content::Base(_)),
        }
    }
    /// Advisory WCAG foreground/background ratio; never modifies colors.
    pub fn contrast(&self) -> ContrastReport {
        self.terminal().palette().contrast()
    }
    /// Preserves VS Code UI overrides even when empty; never silently terminalizes combined drafts.
    pub fn to_toml_fragment(&self) -> Result<String, ThemeError> {
        if self.notes().base_palette_projection {
            return Err(ThemeError::Projection);
        }
        self.to_projected_toml_fragment()
    }
    /// Explicit ANSI16 projection, acknowledging original extra base colors are not in this fragment.
    /// Full original colors remain available through base_scheme().
    pub fn to_projected_toml_fragment(&self) -> Result<String, ThemeError> {
        match &self.content {
            Content::Terminal(s) => s.to_toml_fragment(),
            Content::Combined(s) => s.to_toml_fragment(),
            Content::Base(s) => s.terminal().to_toml_fragment(),
        }
    }
}
/// Single bounded pure import path. JSON name-bearing formats require name=None; nameless
/// formats require a validated explicit display name. No extension sniffing or fallback.
/// Uses existing strict per-format subsets; caller must choose UI base/appearance separately.
pub fn import_theme(
    format: ThemeImportFormat,
    text: &str,
    name: Option<&str>,
) -> Result<ThemeImport, ThemeError> {
    budget(text)?;
    let embedded = matches!(
        format,
        ThemeImportFormat::WindowsTerminal
            | ThemeImportFormat::VsCode
            | ThemeImportFormat::Base16Yaml
            | ThemeImportFormat::Base24Yaml
    );
    if embedded == name.is_some() {
        return Err(ThemeError::NamePolicy);
    }
    if let Some(name) = name {
        validate_name(name)?;
    }
    let content = match format {
        ThemeImportFormat::Base16Yaml => {
            Content::Base(super::import_base_scheme(text, super::BaseSystem::Base16)?)
        }
        ThemeImportFormat::Base24Yaml => {
            Content::Base(super::import_base_scheme(text, super::BaseSystem::Base24)?)
        }
        ThemeImportFormat::WindowsTerminal => Content::Terminal(import_windows_terminal(text)?),
        ThemeImportFormat::VsCode => Content::Combined(import_vscode(text)?),
        ThemeImportFormat::Alacritty => {
            Content::Terminal(import_alacritty(name.ok_or(ThemeError::NamePolicy)?, text)?)
        }
        ThemeImportFormat::Ghostty => {
            Content::Terminal(import_ghostty(name.ok_or(ThemeError::NamePolicy)?, text)?)
        }
        ThemeImportFormat::ItermXml => Content::Terminal(import_itermcolors(
            name.ok_or(ThemeError::NamePolicy)?,
            text,
        )?),
    };
    Ok(ThemeImport { format, content })
}
#[cfg(test)]
#[path = "theme_import_tests.rs"]
mod tests;
