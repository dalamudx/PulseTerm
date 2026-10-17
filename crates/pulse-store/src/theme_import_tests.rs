use super::*;
use crate::theme::{ContrastWarning, MAX_SCHEME_BYTES, parse_terminal_fragment};
fn windows() -> serde_json::Value {
    let mut v = serde_json::json!({"name":"literal/../name 中文","foreground":"#ffffff","background":"#000000"});
    for (i, key) in [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ]
    .iter()
    .enumerate()
    {
        v[*key] = serde_json::json!(format!("#{i:06x}"));
    }
    v
}
fn vscode() -> String {
    let mut colors = serde_json::json!({"terminal.foreground":"#ffffff","terminal.background":"#000000","editor.background":"#112233","editor.selectionBackground":"#11223380"});
    for (i, key) in [
        "Black",
        "Red",
        "Green",
        "Yellow",
        "Blue",
        "Magenta",
        "Cyan",
        "White",
        "BrightBlack",
        "BrightRed",
        "BrightGreen",
        "BrightYellow",
        "BrightBlue",
        "BrightMagenta",
        "BrightCyan",
        "BrightWhite",
    ]
    .iter()
    .enumerate()
    {
        colors[format!("terminal.ansi{key}")] = serde_json::json!(format!("#{i:06x}"));
    }
    serde_json::json!({"name":"VS","colors":colors}).to_string()
}
#[test]
fn metadata_and_literal_name_do_not_become_paths() -> Result<(), Box<dyn std::error::Error>> {
    let r = import_theme(
        ThemeImportFormat::WindowsTerminal,
        &windows().to_string(),
        None,
    )?;
    assert_eq!(r.format(), ThemeImportFormat::WindowsTerminal);
    assert_eq!(r.terminal().name(), "literal/../name 中文");
    assert_eq!(
        r.notes(),
        ThemeImportNotes {
            ui_override_count: 0,
            ignored_editor_color_count: 0,
            base_palette_projection: false,
            rgb8_quantization: false
        }
    );
    assert!(r.ui_overrides().is_none());
    assert!(parse_terminal_fragment(&r.to_toml_fragment()?)? == *r.terminal());
    Ok(())
}
#[test]
fn combined_draft_cannot_lose_ui_overrides() -> Result<(), Box<dyn std::error::Error>> {
    let r = import_theme(ThemeImportFormat::VsCode, &vscode(), None)?;
    assert_eq!(
        r.notes(),
        ThemeImportNotes {
            ui_override_count: 1,
            ignored_editor_color_count: 1,
            base_palette_projection: false,
            rgb8_quantization: false
        }
    );
    assert_eq!(
        r.ui_overrides().and_then(|u| u.get("--bg-base")),
        Some(&Rgb8::new(17, 34, 51))
    );
    assert!(matches!(
        parse_terminal_fragment(&r.to_toml_fragment()?),
        Err(ThemeError::Toml)
    ));
    let text = vscode()
        .replace(",\"editor.background\":\"#112233\"", "")
        .replace("\"editor.background\":\"#112233\",", "");
    let r = import_theme(ThemeImportFormat::VsCode, &text, None)?;
    assert_eq!(r.notes().ui_override_count, 0);
    assert!(r.ui_overrides().is_some());
    assert!(matches!(
        parse_terminal_fragment(&r.to_toml_fragment()?),
        Err(ThemeError::Toml)
    ));
    Ok(())
}
#[test]
fn wrong_name_policy_precedes_parse_no_default_fallback() {
    for f in [
        ThemeImportFormat::WindowsTerminal,
        ThemeImportFormat::VsCode,
    ] {
        assert_eq!(
            import_theme(f, "invalid", Some("ignored-override")).err(),
            Some(ThemeError::NamePolicy)
        );
    }
    for f in [
        ThemeImportFormat::Alacritty,
        ThemeImportFormat::Ghostty,
        ThemeImportFormat::ItermXml,
    ] {
        assert_eq!(
            import_theme(f, "invalid", None).err(),
            Some(ThemeError::NamePolicy)
        );
        assert_eq!(
            import_theme(f, "invalid", Some("\0bad")).err(),
            Some(ThemeError::Name)
        );
    }
}
#[test]
fn explicit_format_never_sniffs_content() {
    let text = windows().to_string();
    assert_eq!(
        import_theme(ThemeImportFormat::Alacritty, &text, Some("Fixture")).err(),
        Some(ThemeError::Alacritty)
    );
    assert_eq!(
        import_theme(ThemeImportFormat::Ghostty, &text, Some("Fixture")).err(),
        Some(ThemeError::Ghostty)
    );
    assert_eq!(
        import_theme(ThemeImportFormat::ItermXml, &text, Some("Fixture")).err(),
        Some(ThemeError::Iterm)
    );
    assert_eq!(
        import_theme(ThemeImportFormat::VsCode, &text, None).err(),
        Some(ThemeError::VsCode)
    );
}
#[test]
fn shared_budget_and_warning_never_modify_colors() -> Result<(), Box<dyn std::error::Error>> {
    for f in [
        ThemeImportFormat::WindowsTerminal,
        ThemeImportFormat::VsCode,
        ThemeImportFormat::Alacritty,
        ThemeImportFormat::Ghostty,
        ThemeImportFormat::ItermXml,
    ] {
        assert_eq!(
            import_theme(f, &" ".repeat(MAX_SCHEME_BYTES + 1), None).err(),
            Some(ThemeError::Budget)
        );
    }
    let mut v = windows();
    v["foreground"] = serde_json::json!("#777777");
    v["background"] = serde_json::json!("#ffffff");
    let r = import_theme(ThemeImportFormat::WindowsTerminal, &v.to_string(), None)?;
    assert_eq!(
        r.contrast().warning,
        Some(ContrastWarning::LowForegroundContrast)
    );
    assert_eq!(r.terminal().palette().foreground, Rgb8::new(119, 119, 119));
    Ok(())
}
