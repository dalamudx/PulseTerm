use super::*;
use crate::theme::{MAX_SCHEME_BYTES, ThemeImportFormat, import_theme, parse_terminal_fragment};
fn imported() -> Result<ThemeImport, ThemeError> {
    let mut v = serde_json::json!({"name":"literal/../Theme 中文","foreground":"#ffffff","background":"#000000"});
    for key in [
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
    ] {
        v[key] = serde_json::json!("#112233");
    }
    import_theme(ThemeImportFormat::WindowsTerminal, &v.to_string(), None)
}
fn fixture() -> Result<ThemeDocument, ThemeError> {
    ThemeDocument::from_import(
        &imported()?,
        ThemeAppearance::Dark,
        UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?),
    )
}
#[test]
fn complete_roundtrip_and_independent_terminal_projection() -> Result<(), Box<dyn std::error::Error>>
{
    let d = fixture()?;
    let t = d.to_toml()?;
    assert!(parse_theme_document(&t)? == d);
    assert_eq!(d.name(), "literal/../Theme 中文");
    assert_eq!(d.appearance(), ThemeAppearance::Dark);
    assert_eq!(d.ui().neutral(), NeutralScale::Slate);
    assert_eq!(d.ui().accent().value(), "cyan");
    assert!(matches!(parse_terminal_fragment(&t), Err(ThemeError::Toml)));
    assert!(parse_theme_document(&d.terminal_scheme()?.to_toml_fragment()?).is_err());
    Ok(())
}
#[test]
fn neutral_appearance_and_accent_choices_are_not_css() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()?.to_toml()?;
    for bad in [
        t.replace("dark", "auto"),
        t.replace("slate", "violet"),
        t.replace("cyan", "Cyan"),
        t.replace("cyan", "url(private-secret)"),
        t.replace("cyan", "#fff"),
        t.replace("cyan", "#01020380"),
    ] {
        assert_eq!(parse_theme_document(&bad).err(), Some(ThemeError::Document));
    }
    for s in SCALES {
        assert_eq!(AccentChoice::parse(s)?.value(), s);
    }
    assert_eq!(AccentChoice::parse("#AbCdEf")?.value(), "#abcdef");
    Ok(())
}
#[test]
fn override_names_are_exact_and_opaque() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()?.to_toml()?;
    for key in ["--bg-base", "--accent", "--info-fg"] {
        let t = t.replace(
            "[ui]",
            &format!("[ui]\noverrides={{\"{key}\"=\"#123456\"}}"),
        );
        assert_eq!(
            parse_theme_document(&t)?.ui().overrides().get(key),
            Some(&Rgb8::new(18, 52, 86))
        );
    }
    for key in [
        "--font-ui",
        "--space-1",
        "--private",
        "--info-foo",
        "--bg-base ",
    ] {
        let t = t.replace(
            "[ui]",
            &format!("[ui]\noverrides={{\"{key}\"=\"#123456\"}}"),
        );
        assert_eq!(parse_theme_document(&t).err(), Some(ThemeError::Document));
    }
    let invalid = t.replace(
        "[ui]",
        "[ui]\noverrides={\"--bg-base\"=\"url(private-secret)\"}",
    );
    let error = parse_theme_document(&invalid).err().ok_or("accepted")?;
    assert!(!error.to_string().contains("private-secret"));
    Ok(())
}
#[test]
fn missing_duplicate_and_extra_fields_reject() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()?.to_toml()?;
    for bad in [
        t.replace("appearance = \"dark\"\n", ""),
        t.replace("[ui]", "[ui]\naccent=\"red\""),
        t.clone() + "\n[unknown]\nkey=1\n",
        t.replace("[ui]", "[ui]\npath=\"/private\""),
        t.replace("name =", "name2 ="),
    ] {
        assert_eq!(parse_theme_document(&bad).err(), Some(ThemeError::Document));
    }
    assert_eq!(
        parse_theme_document(&" ".repeat(MAX_SCHEME_BYTES + 1)).err(),
        Some(ThemeError::Budget)
    );
    Ok(())
}
#[test]
fn imported_ui_overrides_are_preserved_without_inferred_base()
-> Result<(), Box<dyn std::error::Error>> {
    let i = imported()?;
    let mut palette = serde_json::Map::new();
    palette.insert("terminal.foreground".into(), serde_json::json!("#ffffff"));
    palette.insert("terminal.background".into(), serde_json::json!("#000000"));
    palette.insert("editor.background".into(), serde_json::json!("#aabbcc"));
    for key in [
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
    ] {
        palette.insert(format!("terminal.ansi{key}"), serde_json::json!("#112233"));
    }
    let imported = import_theme(
        ThemeImportFormat::VsCode,
        &serde_json::json!({"name":i.terminal().name(),"colors":palette}).to_string(),
        None,
    )?;
    let d = ThemeDocument::from_import(
        &imported,
        ThemeAppearance::Light,
        UiThemeBase::new(NeutralScale::Sand, AccentChoice::parse("#123456")?),
    )?;
    assert_eq!(d.ui().accent().value(), "#123456");
    assert_eq!(d.ui().neutral(), NeutralScale::Sand);
    assert_eq!(
        d.ui().overrides().get("--bg-base"),
        Some(&Rgb8::new(170, 187, 204))
    );
    assert!(parse_theme_document(&d.to_toml()?)? == d);
    assert!(*d.terminal() == *i.terminal().palette());
    Ok(())
}
