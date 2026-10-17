use super::*;
use crate::theme::{ContrastWarning, MAX_SCHEME_BYTES, parse_terminal_fragment};
fn fixture() -> String {
    let mut t = "# synthetic only\nforeground = FFFFFF\nbackground = #000000\n".to_owned();
    for i in (0..16).rev() {
        t.push_str(&format!("palette = {i} = #{i:06x}\n"));
    }
    t
}
#[test]
fn explicit_indices_and_hex_forms_preserve_all_slots() -> Result<(), Box<dyn std::error::Error>> {
    let s = import_ghostty("Ghostty 中文", &fixture())?;
    for (i, c) in s.palette().ansi().iter().enumerate() {
        assert_eq!(usize::from(c.channels()[2]), i);
    }
    assert_eq!(s.palette().foreground, Rgb8::new(255, 255, 255));
    assert_eq!(s.palette().cursor, None);
    assert_eq!(s.name(), "Ghostty 中文");
    assert!(parse_terminal_fragment(&s.to_toml_fragment()?)? == s);
    Ok(())
}
#[test]
fn all_optional_fields_map_and_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let s = import_ghostty(
        "Fixture",
        &(fixture()
            + "cursor-color=ABCDEF\ncursor-text=#123456\nselection-background=102030\nselection-foreground=#405060\n"),
    )?;
    assert_eq!(s.palette().cursor, Some(Rgb8::new(171, 205, 239)));
    assert_eq!(s.palette().cursor_text, Some(Rgb8::new(18, 52, 86)));
    assert_eq!(
        s.palette().selection_background,
        Some(Rgb8::new(16, 32, 48))
    );
    assert_eq!(
        s.palette().selection_foreground,
        Some(Rgb8::new(64, 80, 96))
    );
    assert!(parse_terminal_fragment(&s.to_toml_fragment()?)? == s);
    Ok(())
}
#[test]
fn comments_whitespace_and_crlf_do_not_change_scheme() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture();
    let s = import_ghostty("Fixture", &t)?;
    assert!(
        import_ghostty(
            "Fixture",
            &("  # foreground=123456\r\n\r\n".to_owned() + &t.replace('\n', "\r\n"))
        )? == s
    );
    Ok(())
}
#[test]
fn duplicates_are_not_silently_last_wins() {
    for extra in [
        "foreground=FFFFFF\n",
        "palette=0=000000\n",
        "cursor-color=123456\ncursor-color=123456\n",
        "palette=00=000000\n",
    ] {
        assert!(matches!(
            import_ghostty("Fixture", &(fixture() + extra)),
            Err(ThemeError::Ghostty)
        ));
    }
}
#[test]
fn required_fields_and_indices_are_complete() {
    for t in [
        fixture().replace("foreground = FFFFFF\n", ""),
        fixture().replace("background = #000000\n", ""),
        fixture().replace("palette = 5 = #000005\n", ""),
    ] {
        assert!(matches!(
            import_ghostty("Fixture", &t),
            Err(ThemeError::Ghostty)
        ));
    }
    for i in ["16", "255", "-1", "+0", "١", "9999999999999999"] {
        assert!(matches!(
            import_ghostty("Fixture", &(fixture() + &format!("palette={i}=000000\n"))),
            Err(ThemeError::Ghostty)
        ));
    }
}
#[test]
fn dynamic_css_alpha_and_non_color_directives_are_rejected() {
    for extra in [
        "theme=other\n",
        "config-file=private-input-marker\n",
        "font-size=14\n",
        "foreground\n",
        "cursor-color=CellForeground\n",
        "cursor-color=rgba(1,2,3,1)\n",
        "cursor-color=#aabbccdd\n",
        "cursor-color=#ffffff # inline not supported\n",
    ] {
        let error = import_ghostty("Fixture", &(fixture() + extra)).err();
        assert_eq!(error, Some(ThemeError::Ghostty));
        assert!(
            !error
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("private-input-marker")
        );
    }
}
#[test]
fn byte_and_name_budgets_precede_conversion() {
    assert!(matches!(
        import_ghostty("Fixture", &" ".repeat(MAX_SCHEME_BYTES + 1)),
        Err(ThemeError::Budget)
    ));
    for name in ["", " ", "a\0b", "a\nb"] {
        assert!(matches!(
            import_ghostty(name, &fixture()),
            Err(ThemeError::Name)
        ));
    }
    assert!(matches!(
        import_ghostty(&"中".repeat(43), &fixture()),
        Err(ThemeError::Name)
    ));
}
#[test]
fn contrast_warning_does_not_change_input_colors() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()
        .replace("foreground = FFFFFF", "foreground = 777777")
        .replace("background = #000000", "background = #FFFFFF");
    let s = import_ghostty("Fixture", &t)?;
    assert_eq!(
        s.palette().contrast().warning,
        Some(ContrastWarning::LowForegroundContrast)
    );
    assert_eq!(s.palette().foreground, Rgb8::new(119, 119, 119));
    Ok(())
}
