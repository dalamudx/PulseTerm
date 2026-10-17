use super::*;
use crate::theme::{
    AccentChoice, ThemeImportFormat, UiThemeBase, import_theme, parse_theme_document,
};
fn fixture(
    name: &str,
    appearance: ThemeAppearance,
    accent: &str,
) -> Result<ThemeDocument, ThemeError> {
    let mut input = serde_json::json!({"name":name,"foreground":"#ffffff","background":"#000000"});
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
        input[*key] = serde_json::json!(format!("#{i:02x}80ff"));
    }
    ThemeDocument::from_import(
        &import_theme(ThemeImportFormat::WindowsTerminal, &input.to_string(), None)?,
        appearance,
        UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse(accent)?),
    )
}
#[test]
fn all_terminal_channels_and_optional_colors_preserved() -> Result<(), Box<dyn std::error::Error>> {
    let raw = format!(
        "{}\ncursor='#abcdef'\ncursorText='#010203'\nselectionBackground='#445566'\nselectionForeground='#778899'\n",
        fixture("channel fixture", ThemeAppearance::Dark, "cyan")?.to_toml()?
    );
    let document = parse_theme_document(&raw)?;
    let paint = document.paint_data()?;
    let terminal = paint.terminal();
    assert_eq!(terminal.foreground(), [255, 255, 255]);
    assert_eq!(terminal.background(), [0, 0, 0]);
    for (i, rgb) in terminal.ansi().iter().enumerate() {
        assert_eq!(*rgb, [u8::try_from(i)?, 128, 255]);
    }
    assert_eq!(terminal.cursor(), Some([171, 205, 239]));
    assert_eq!(terminal.cursor_text(), Some([1, 2, 3]));
    assert_eq!(terminal.selection_background(), Some([68, 85, 102]));
    assert_eq!(terminal.selection_foreground(), Some([119, 136, 153]));
    Ok(())
}
#[test]
fn missing_optional_colors_are_not_invented() -> Result<(), Box<dyn std::error::Error>> {
    let p = fixture("absent", ThemeAppearance::Light, "red")?.paint_data()?;
    assert_eq!(p.terminal().cursor(), None);
    assert_eq!(p.terminal().cursor_text(), None);
    assert_eq!(p.terminal().selection_background(), None);
    assert_eq!(p.terminal().selection_foreground(), None);
    let v: serde_json::Value = serde_json::from_str(&p.to_json()?)?;
    for field in [
        "cursor",
        "cursorText",
        "selectionBackground",
        "selectionForeground",
    ] {
        assert!(v["terminal"].get(field).is_none());
    }
    Ok(())
}
#[test]
fn every_fixed_ui_slot_survives_under_the_output_budget() -> Result<(), Box<dyn std::error::Error>>
{
    let keys = [
        "--bg-base",
        "--bg-subtle",
        "--bg-surface",
        "--bg-hover",
        "--bg-active",
        "--border-subtle",
        "--border",
        "--border-strong",
        "--fg-muted",
        "--fg",
        "--accent-bg",
        "--accent-bg-hover",
        "--accent",
        "--accent-hover",
        "--accent-contrast",
        "--accent-fg",
        "--danger-bg",
        "--danger",
        "--danger-fg",
        "--warning-bg",
        "--warning",
        "--warning-fg",
        "--success-bg",
        "--success",
        "--success-fg",
        "--info-bg",
        "--info",
        "--info-fg",
    ];
    let mut raw = fixture("PRIVATE label /../", ThemeAppearance::Dark, "#ABCDEF")?.to_toml()?;
    raw.push_str("\n[ui.overrides]\n");
    for key in keys {
        raw.push_str(&format!("'{key}'='#ffffff'\n"));
    }
    let p = parse_theme_document(&raw)?.paint_data()?;
    assert_eq!(p.overrides().len(), 28);
    let actual = p
        .overrides()
        .iter()
        .map(|c| {
            assert_eq!(c.color(), [255, 255, 255]);
            c.slot().css_variable()
        })
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(actual, keys.into_iter().collect());
    let encoded = p.to_json()?;
    assert!(encoded.len() <= MAX_THEME_PAINT_BYTES);
    assert!(!encoded.contains("PRIVATE") && !encoded.contains("--") && !encoded.contains("name"));
    Ok(())
}
#[test]
fn names_and_source_text_never_enter_paint_data() -> Result<(), Box<dyn std::error::Error>> {
    let a = fixture(
        "PRIVATE </style><script>/../中文",
        ThemeAppearance::Dark,
        "blue",
    )?;
    let b = fixture("another label", ThemeAppearance::Dark, "blue")?;
    assert!(a != b);
    assert!(a.paint_data()? == b.paint_data()?);
    assert_eq!(a.paint_data()?.to_json()?, b.paint_data()?.to_json()?);
    let value: serde_json::Value = serde_json::from_str(&a.paint_data()?.to_json()?)?;
    assert_eq!(
        value
            .as_object()
            .ok_or("not object")?
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        [
            "version",
            "accent",
            "appearance",
            "neutral",
            "overrides",
            "terminal",
            "lowForegroundContrast"
        ]
        .into_iter()
        .collect()
    );
    Ok(())
}
#[test]
fn appearance_and_canonical_seed_are_explicit_not_resolved()
-> Result<(), Box<dyn std::error::Error>> {
    for appearance in [ThemeAppearance::Light, ThemeAppearance::Dark] {
        let p = fixture("selection", appearance, "#ABCDEF")?.paint_data()?;
        assert_eq!(p.appearance(), appearance);
        assert_eq!(p.neutral(), NeutralScale::Slate);
        assert_eq!(p.accent(), "#abcdef");
        assert!(p.overrides().is_empty());
    }
    assert!(matches!(
        fixture("unsafe", ThemeAppearance::Dark, "url(x)"),
        Err(ThemeError::Document)
    ));
    Ok(())
}
#[test]
fn unknown_variables_fail_closed_not_passed_as_css() -> Result<(), Box<dyn std::error::Error>> {
    assert!(matches!(
        UiColorSlot::from_variable("--bg-base; color:red"),
        Err(ThemeError::Document)
    ));
    let raw = format!(
        "{}\n[ui.overrides]\n'--arbitrary-color'='#123456'\n",
        fixture("bad slot", ThemeAppearance::Dark, "blue")?.to_toml()?
    );
    assert!(matches!(
        parse_theme_document(&raw),
        Err(ThemeError::Document)
    ));
    Ok(())
}
#[test]
fn contrast_advisory_preserves_colors_and_unrounded_threshold()
-> Result<(), Box<dyn std::error::Error>> {
    for (foreground, expected) in [("#767676", false), ("#777777", true), ("#ffffff", true)] {
        let raw = fixture("contrast", ThemeAppearance::Light, "blue")?
            .to_toml()?
            .replace(
                "foreground = \"#ffffff\"",
                &format!("foreground = \"{foreground}\""),
            )
            .replace("background = \"#000000\"", "background = \"#ffffff\"");
        let d = parse_theme_document(&raw)?;
        let p = d.paint_data()?;
        assert_eq!(p.low_foreground_contrast(), expected);
        assert_eq!(
            p.terminal().foreground(),
            d.terminal().foreground.channels()
        );
        let v: serde_json::Value = serde_json::from_str(&p.to_json()?)?;
        assert_eq!(v["lowForegroundContrast"], serde_json::json!(expected));
    }
    Ok(())
}
