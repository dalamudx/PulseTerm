use super::*;
fn fixture() -> serde_json::Value {
    let mut v = serde_json::json!({"name":"Fixture 中文", "foreground":"#FFFFFF", "background":"#000000", "cursorColor":"#AAbBcC", "selectionBackground":"#123456"});
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
        v[*key] = serde_json::Value::String(format!("#{i:06x}"));
    }
    v
}
fn scheme() -> Result<TerminalScheme, Box<dyn std::error::Error>> {
    Ok(import_windows_terminal(&fixture().to_string())?)
}
#[test]
fn ansi_order_cursor_mapping_and_lossless_fragment() -> Result<(), Box<dyn std::error::Error>> {
    let s = scheme()?;
    assert_eq!(s.name(), "Fixture 中文");
    for (i, c) in s.palette().ansi().iter().enumerate() {
        assert_eq!(usize::from(c.channels()[2]), i);
    }
    assert_eq!(s.palette().cursor, Some(Rgb8::new(170, 187, 204)));
    assert_eq!(s.palette().cursor_text, None);
    assert_eq!(
        s.palette().selection_background,
        Some(Rgb8::new(18, 52, 86))
    );
    assert_eq!(s.palette().selection_foreground, None);
    let t = s.to_toml_fragment()?;
    assert!(t.contains("cursor = \"#aabbcc\""));
    assert!(!t.contains("cursorColor"));
    let restored = parse_terminal_fragment(&t)?;
    assert!(restored == s);
    Ok(())
}
#[test]
fn optional_colors_are_not_invented() -> Result<(), Box<dyn std::error::Error>> {
    let mut v = fixture();
    v.as_object_mut().ok_or("object")?.remove("cursorColor");
    v.as_object_mut()
        .ok_or("object")?
        .remove("selectionBackground");
    let s = import_windows_terminal(&v.to_string())?;
    assert_eq!(s.palette().cursor, None);
    assert_eq!(s.palette().selection_background, None);
    Ok(())
}
#[test]
fn rgb_codec_is_opaque_and_canonical() -> Result<(), Box<dyn std::error::Error>> {
    for r in [0, 1, 127, 255] {
        for g in [0, 1, 127, 255] {
            for b in [0, 1, 127, 255] {
                let c = Rgb8::new(r, g, b);
                let text: String = c.into();
                assert_eq!(Rgb8::parse(&text)?, c);
                assert_eq!(
                    serde_json::from_str::<Rgb8>(&serde_json::to_string(&c)?)?,
                    c
                );
            }
        }
    }
    for bad in [
        "red",
        "#fff",
        "#ffffff00",
        "rgb(0,0,0)",
        "var(--bg)",
        "url(secret)",
        "#fffffg",
        "#ｆfffff",
        " #ffffff",
        "#ffffff\n",
        "",
    ] {
        assert_eq!(Rgb8::parse(bad), Err(ThemeError::Color));
    }
    Ok(())
}
#[test]
fn wcag_reference_ratios_threshold_and_no_adjustment() -> Result<(), Box<dyn std::error::Error>> {
    let mut p = scheme()?.palette().clone();
    assert!((p.contrast().ratio - 21.0).abs() < 1e-12);
    assert_eq!(p.contrast().warning, None);
    p.foreground = p.background;
    assert!((p.contrast().ratio - 1.0).abs() < 1e-12);
    assert_eq!(
        p.contrast().warning,
        Some(ContrastWarning::LowForegroundContrast)
    );
    p.background = Rgb8::new(255, 255, 255);
    p.foreground = Rgb8::new(119, 119, 119);
    let ratio = p.contrast().ratio;
    assert!((ratio - 4.478089453577214).abs() < 1e-12);
    assert!(p.contrast().warning.is_some());
    assert_eq!(p.foreground.channels(), [119; 3]);
    p.foreground = Rgb8::new(118, 118, 118);
    assert!(p.contrast().ratio > 4.5);
    assert_eq!(p.contrast().warning, None);
    std::mem::swap(&mut p.foreground, &mut p.background);
    assert!(p.contrast().ratio > 4.5);
    Ok(())
}
#[test]
fn names_preserve_literal_quotes_unicode_without_paths() -> Result<(), Box<dyn std::error::Error>> {
    let mut v = fixture();
    v["name"] = serde_json::json!("../Literal \"主题\" # [ui]");
    let s = import_windows_terminal(&v.to_string())?;
    assert_eq!(s.name(), "../Literal \"主题\" # [ui]");
    assert!(parse_terminal_fragment(&s.to_toml_fragment()?)? == s);
    Ok(())
}
#[test]
fn invalid_names_rejected() -> Result<(), Box<dyn std::error::Error>> {
    for name in [
        "".to_owned(),
        "  ".to_owned(),
        "a\nb".to_owned(),
        "a\0b".to_owned(),
        "中".repeat(43),
    ] {
        let mut v = fixture();
        v["name"] = serde_json::Value::String(name);
        assert!(matches!(
            import_windows_terminal(&v.to_string()),
            Err(ThemeError::Name)
        ));
    }
    Ok(())
}
#[test]
fn strict_json_missing_unknown_duplicate_and_css_rejected() -> Result<(), Box<dyn std::error::Error>>
{
    let text = fixture().to_string();
    let duplicate = text.replacen('{', "{\"name\":\"duplicate\",", 1);
    assert!(matches!(
        import_windows_terminal(&duplicate),
        Err(ThemeError::Json)
    ));
    for key in ["cursor", "unknown", "schemes", "ui"] {
        let mut v = fixture();
        v[key] = serde_json::json!("#ffffff");
        assert!(matches!(
            import_windows_terminal(&v.to_string()),
            Err(ThemeError::Json)
        ));
    }
    let mut v = fixture();
    v.as_object_mut().ok_or("object")?.remove("brightWhite");
    assert!(matches!(
        import_windows_terminal(&v.to_string()),
        Err(ThemeError::Json)
    ));
    v = fixture();
    v["red"] = serde_json::json!("url(secret-theme-marker)");
    let error = import_windows_terminal(&v.to_string())
        .err()
        .ok_or("must reject")?;
    assert_eq!(error, ThemeError::Json);
    assert!(!error.to_string().contains("secret-theme-marker"));
    Ok(())
}
#[test]
fn terminal_fragment_does_not_accept_ui_or_duplicates() -> Result<(), Box<dyn std::error::Error>> {
    let t = scheme()?.to_toml_fragment()?;
    for extra in [
        "\n[ui]\nneutral=\"slate\"\n",
        "\nforeground=\"#000000\"\n",
        "\nunknown=1\n",
    ] {
        assert!(matches!(
            parse_terminal_fragment(&(t.clone() + extra)),
            Err(ThemeError::Toml)
        ));
    }
    Ok(())
}
#[test]
fn input_budget_before_parsing() -> Result<(), Box<dyn std::error::Error>> {
    let b = " ".repeat(MAX_SCHEME_BYTES + 1);
    assert!(matches!(
        import_windows_terminal(&b),
        Err(ThemeError::Budget)
    ));
    assert!(matches!(
        parse_terminal_fragment(&b),
        Err(ThemeError::Budget)
    ));
    let mut t = fixture().to_string();
    t.push_str(&" ".repeat(MAX_SCHEME_BYTES - t.len()));
    assert!(import_windows_terminal(&t).is_ok());
    Ok(())
}
