use super::*;
use crate::theme::{ContrastWarning, MAX_SCHEME_BYTES, parse_terminal_fragment};
fn fixture() -> String {
    let mut text = "[colors.primary]\nforeground='#FFFFFF'\nbackground='0x000000'\n".to_owned();
    for (table, offset) in [("normal", 0), ("bright", 8)] {
        text.push_str(&format!("[colors.{table}]\n"));
        for (i, k) in [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ]
        .iter()
        .enumerate()
        {
            text.push_str(&format!("{k}='0x{:06x}'\n", i + offset));
        }
    }
    text
}
#[test]
fn ansi_mapping_hex_prefix_and_fragment_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let s = import_alacritty("Alacritty 中文", &fixture())?;
    for (i, c) in s.palette().ansi().iter().enumerate() {
        assert_eq!(usize::from(c.channels()[2]), i);
    }
    assert_eq!(s.palette().foreground, Rgb8::new(255, 255, 255));
    assert_eq!(s.palette().cursor, None);
    assert!(parse_terminal_fragment(&s.to_toml_fragment()?)? == s);
    assert_eq!(s.name(), "Alacritty 中文");
    Ok(())
}
#[test]
fn all_optional_cursor_and_selection_colors_map() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()
        + "[colors.cursor]\ncursor='#ABCDEF'\ntext='0x123456'\n[colors.selection]\nbackground='#102030'\ntext='#405060'\n";
    let s = import_alacritty("Fixture", &t)?;
    let p = s.palette();
    assert_eq!(p.cursor, Some(Rgb8::new(171, 205, 239)));
    assert_eq!(p.cursor_text, Some(Rgb8::new(18, 52, 86)));
    assert_eq!(p.selection_background, Some(Rgb8::new(16, 32, 48)));
    assert_eq!(p.selection_foreground, Some(Rgb8::new(64, 80, 96)));
    assert!(parse_terminal_fragment(&s.to_toml_fragment()?)? == s);
    Ok(())
}
#[test]
fn sparse_optional_tables_do_not_invent_defaults() -> Result<(), Box<dyn std::error::Error>> {
    let s = import_alacritty(
        "Fixture",
        &(fixture() + "[colors.cursor]\ntext='#010203'\n[colors.selection]\n"),
    )?;
    assert_eq!(s.palette().cursor, None);
    assert_eq!(s.palette().cursor_text, Some(Rgb8::new(1, 2, 3)));
    assert_eq!(s.palette().selection_foreground, None);
    assert_eq!(s.palette().selection_background, None);
    Ok(())
}
#[test]
fn missing_required_and_duplicate_fields_reject() {
    for t in [
        fixture().replacen("magenta='0x000005'\n", "", 1),
        fixture().replace("foreground='#FFFFFF'\n", ""),
        fixture() + "[colors.normal]\nred='#123456'\n",
        fixture().replacen(
            "foreground='#FFFFFF'",
            "foreground='#FFFFFF'\nforeground='#000000'",
            1,
        ),
    ] {
        assert!(matches!(
            import_alacritty("Fixture", &t),
            Err(ThemeError::Alacritty)
        ));
    }
}
#[test]
fn unrelated_config_and_unrepresentable_extensions_reject() {
    for extra in [
        "[font]\nsize=14\n",
        "[colors.dim]\nblack='#010203'\n",
        "[[colors.indexed_colors]]\nindex=16\ncolor='#010203'\n",
    ] {
        assert!(matches!(
            import_alacritty("Fixture", &(fixture() + extra)),
            Err(ThemeError::Alacritty)
        ));
    }
}
#[test]
fn dynamic_alpha_and_css_colors_are_not_approximated() {
    for color in [
        "CellForeground",
        "CellBackground",
        "0xffabcdef",
        "#ABC",
        "rgba(1,2,3,1)",
        "url(private-palette-marker)",
    ] {
        let text = fixture() + &format!("[colors.cursor]\ncursor='{color}'\n");
        let error = import_alacritty("Fixture", &text).err();
        assert_eq!(error, Some(ThemeError::Alacritty));
        assert!(
            !error
                .map(|e| e.to_string())
                .unwrap_or_default()
                .contains("private-palette-marker")
        );
    }
}
#[test]
fn independent_name_and_input_budgets_are_checked() {
    for name in ["", "  ", "a\0b", "a\nb"] {
        assert!(matches!(
            import_alacritty(name, &fixture()),
            Err(ThemeError::Name)
        ));
    }
    assert!(matches!(
        import_alacritty(&"中".repeat(43), &fixture()),
        Err(ThemeError::Name)
    ));
    assert!(matches!(
        import_alacritty("Fixture", &" ".repeat(MAX_SCHEME_BYTES + 1)),
        Err(ThemeError::Budget)
    ));
}
#[test]
fn shared_wcag_warning_preserves_supplied_colors() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()
        .replace("foreground='#FFFFFF'", "foreground='#777777'")
        .replace("background='0x000000'", "background='#FFFFFF'");
    let s = import_alacritty("Fixture", &t)?;
    assert_eq!(
        s.palette().contrast().warning,
        Some(ContrastWarning::LowForegroundContrast)
    );
    assert_eq!(s.palette().foreground, Rgb8::new(119, 119, 119));
    Ok(())
}
