use super::*;
use crate::theme::{ContrastWarning, MAX_SCHEME_BYTES, parse_terminal_fragment};
fn fixture() -> serde_json::Value {
    let mut colors =
        serde_json::json!({"terminal.foreground":"#FFFFFF","terminal.background":"#000000"});
    for (i, key) in ANSI.iter().enumerate() {
        colors[*key] = serde_json::json!(format!("#{i:06x}"));
    }
    serde_json::json!({"name":"VS 中文", "colors":colors})
}
#[test]
fn complete_ansi_slots_name_and_terminal_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let t = import_vscode(&fixture().to_string())?;
    assert_eq!(t.terminal().name(), "VS 中文");
    for (i, c) in t.terminal().palette().ansi().iter().enumerate() {
        assert_eq!(usize::from(c.channels()[2]), i);
    }
    assert_eq!(t.ignored_color_count(), 0);
    assert!(t.ui_overrides().is_empty());
    assert!(parse_terminal_fragment(&t.terminal().to_toml_fragment()?)? == *t.terminal());
    Ok(())
}
#[test]
fn extracts_ui_and_terminal_together_with_fixed_variable_names()
-> Result<(), Box<dyn std::error::Error>> {
    let mut v = fixture();
    for (i, (key, _)) in UI.iter().enumerate() {
        v["colors"][*key] = serde_json::json!(format!("#{i:06x}"));
    }
    let t = import_vscode(&v.to_string())?;
    for (i, (_, key)) in UI.iter().enumerate() {
        assert_eq!(t.ui_overrides().get(key), Some(&Rgb8::new(0, 0, i as u8)));
    }
    let fragment = t.to_toml_fragment()?;
    let parsed: toml::Value = toml::from_str(&fragment)?;
    assert_eq!(
        parsed["ui"]["overrides"]["--accent"].as_str(),
        Some("#000004")
    );
    assert_eq!(parsed["terminal"]["purple"].as_str(), Some("#000005"));
    assert!(matches!(
        parse_terminal_fragment(&fragment),
        Err(ThemeError::Toml)
    ));
    assert!(parsed.get("appearance").is_none());
    Ok(())
}
#[test]
fn cursor_and_selection_keys_map_without_invented_defaults()
-> Result<(), Box<dyn std::error::Error>> {
    let mut v = fixture();
    for (k, c) in [
        ("terminalCursor.foreground", "#010203"),
        ("terminalCursor.background", "#040506"),
        ("terminal.selectionBackground", "#070809"),
        ("terminal.selectionForeground", "#0a0b0c"),
    ] {
        v["colors"][k] = serde_json::json!(c);
    }
    let t = import_vscode(&v.to_string())?;
    let p = t.terminal().palette();
    assert_eq!(p.cursor, Some(Rgb8::new(1, 2, 3)));
    assert_eq!(p.cursor_text, Some(Rgb8::new(4, 5, 6)));
    assert_eq!(p.selection_background, Some(Rgb8::new(7, 8, 9)));
    assert_eq!(p.selection_foreground, Some(Rgb8::new(10, 11, 12)));
    assert_eq!(
        import_vscode(&fixture().to_string())?
            .terminal()
            .palette()
            .cursor,
        None
    );
    Ok(())
}
#[test]
fn known_metadata_and_token_grammars_are_not_applied() -> Result<(), Box<dyn std::error::Error>> {
    let mut v = fixture();
    v["$schema"] = serde_json::json!("https://untrusted.invalid/never-fetch");
    v["type"] = serde_json::json!("dark");
    v["semanticHighlighting"] = serde_json::json!(true);
    v["tokenColors"] = serde_json::json!([{"scope":"comment","settings":{"foreground":"url(private-input-marker)"}}]);
    v["semanticTokenColors"] = serde_json::json!({"function":"#ffffff"});
    v["colors"]["editor.selectionBackground"] = serde_json::json!("#12345678");
    let t = import_vscode(&v.to_string())?;
    assert_eq!(t.ignored_color_count(), 1);
    assert!(t.ui_overrides().is_empty());
    assert!(!t.to_toml_fragment()?.contains("private-input-marker"));
    Ok(())
}
#[test]
fn includes_jsonc_and_unknown_top_fields_reject() {
    for extra in ["include", "unknown", "--bg-base"] {
        let mut v = fixture();
        v[extra] = serde_json::json!("private-input-marker");
        assert_eq!(
            import_vscode(&v.to_string()).err(),
            Some(ThemeError::VsCode)
        );
    }
    assert_eq!(
        import_vscode(&("// comment\n".to_owned() + &fixture().to_string())).err(),
        Some(ThemeError::VsCode)
    );
}
#[test]
fn missing_required_and_unsupported_terminal_keys_reject() {
    for key in [
        "terminal.foreground",
        "terminal.background",
        ANSI[5],
        ANSI[13],
    ] {
        let mut v = fixture();
        if let Some(colors) = v["colors"].as_object_mut() {
            colors.remove(key);
        } else {
            panic!("fixture colors missing");
        }
        assert_eq!(
            import_vscode(&v.to_string()).err(),
            Some(ThemeError::VsCode)
        );
    }
    for key in [
        "terminal.border",
        "terminal.inactiveSelectionBackground",
        "terminalCursor.unknown",
    ] {
        let mut v = fixture();
        v["colors"][key] = serde_json::json!("#123456");
        assert_eq!(
            import_vscode(&v.to_string()).err(),
            Some(ThemeError::VsCode)
        );
    }
}
#[test]
fn duplicate_colors_even_escaped_and_unmapped_are_rejected() {
    let s = fixture().to_string();
    for key in ["terminal.foreground", "terminal.\\u0066oreground"] {
        let t = s.replacen(
            "\"colors\":{",
            &format!("\"colors\":{{\"{key}\":\"#ffffff\","),
            1,
        );
        assert_eq!(import_vscode(&t).err(), Some(ThemeError::VsCode));
    }
    let t = s.replacen(
        "\"colors\":{",
        "\"colors\":{\"ignored\":\"a\",\"ignored\":\"b\",",
        1,
    );
    assert_eq!(import_vscode(&t).err(), Some(ThemeError::VsCode));
    let t = s.replacen("\"name\":", "\"name\":\"duplicate\",\"name\":", 1);
    assert_eq!(import_vscode(&t).err(), Some(ThemeError::VsCode));
}
#[test]
fn opaque_extracted_colors_only_and_errors_do_not_leak() {
    for key in [
        "terminal.foreground",
        "editor.background",
        "terminalCursor.foreground",
    ] {
        for value in [
            "#ffffff80",
            "CellForeground",
            "url(private-input-marker)",
            "#fff",
            "",
        ] {
            let mut v = fixture();
            v["colors"][key] = serde_json::json!(value);
            let error = import_vscode(&v.to_string()).err();
            assert_eq!(error, Some(ThemeError::VsCode));
            assert!(
                !error
                    .map(|e| e.to_string())
                    .unwrap_or_default()
                    .contains("private-input-marker")
            );
        }
    }
}
#[test]
fn limits_cover_bytes_depth_and_color_count() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        import_vscode(&" ".repeat(MAX_SCHEME_BYTES + 1)).err(),
        Some(ThemeError::Budget)
    );
    let mut v = fixture();
    for i in 0..494 {
        v["colors"][format!("unmapped.{i}")] = serde_json::json!("#000000");
    }
    assert_eq!(import_vscode(&v.to_string())?.ignored_color_count(), 494);
    v["colors"]["too-many"] = serde_json::json!("#000000");
    assert_eq!(
        import_vscode(&v.to_string()).err(),
        Some(ThemeError::VsCode)
    );
    let mut v = fixture();
    v["tokenColors"] = serde_json::from_str(&format!("{}0{}", "[".repeat(32), "]".repeat(32)))?;
    assert_eq!(
        import_vscode(&v.to_string()).err(),
        Some(ThemeError::VsCode)
    );
    v["tokenColors"] = serde_json::json!("{{[[ escaped \\\" braces");
    assert!(import_vscode(&v.to_string()).is_ok());
    Ok(())
}
#[test]
fn names_and_contrast_use_shared_validation() -> Result<(), Box<dyn std::error::Error>> {
    for name in ["", "a\0b", "a\nb"] {
        let mut v = fixture();
        v["name"] = serde_json::json!(name);
        assert_eq!(import_vscode(&v.to_string()).err(), Some(ThemeError::Name));
    }
    let mut v = fixture();
    v["colors"]["terminal.foreground"] = serde_json::json!("#777777");
    v["colors"]["terminal.background"] = serde_json::json!("#ffffff");
    let t = import_vscode(&v.to_string())?;
    assert_eq!(
        t.terminal().palette().contrast().warning,
        Some(ContrastWarning::LowForegroundContrast)
    );
    assert_eq!(t.terminal().palette().foreground, Rgb8::new(119, 119, 119));
    Ok(())
}
