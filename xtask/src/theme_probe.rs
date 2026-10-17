use anyhow::{Result, ensure};
use pulse_store::theme::{
    AccentChoice, ContrastWarning, NeutralScale, Rgb8, ThemeAppearance, ThemeDocument, ThemeError,
    ThemeImport, ThemeImportFormat, UiThemeBase, import_alacritty, import_ghostty,
    import_itermcolors, import_theme, import_vscode, import_windows_terminal,
    parse_terminal_fragment, parse_theme_document,
};
pub(super) fn run() -> Result<()> {
    run_inner(false)
}
pub(super) fn run_json() -> Result<()> {
    run_inner(true)
}
fn run_inner(emit: bool) -> Result<()> {
    // Synthetic colors only; no downloaded catalog, user files or third-party palette.
    let mut value = serde_json::json!({"name":"Fixed ANSI 中文", "foreground":"#FFFFFF", "background":"#000000", "cursorColor":"#ABCDEF"});
    for (i, k) in [
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
        value[*k] = serde_json::Value::String(format!("#{i:06x}"));
    }
    let scheme = import_windows_terminal(&value.to_string())?;
    for (i, c) in scheme.palette().ansi().iter().enumerate() {
        ensure!(usize::from(c.channels()[2]) == i, "ANSI index mismatch");
    }
    ensure!(
        scheme.palette().cursor == Some(Rgb8::new(171, 205, 239)),
        "cursor mapping mismatch"
    );
    ensure!(
        scheme.palette().cursor_text.is_none() && scheme.palette().selection_foreground.is_none(),
        "invented optional color"
    );
    let fragment = scheme.to_toml_fragment()?;
    ensure!(
        parse_terminal_fragment(&fragment)? == scheme,
        "terminal fragment mismatch"
    );
    ensure!(
        (scheme.palette().contrast().ratio - 21.0).abs() < 1e-12
            && scheme.palette().contrast().warning.is_none(),
        "WCAG reference mismatch"
    );
    let mut alacritty =
        "[colors.primary]\nforeground='#FFFFFF'\nbackground='0x000000'\n".to_owned();
    for (table, offset) in [("normal", 0), ("bright", 8)] {
        alacritty.push_str(&format!("[colors.{table}]\n"));
        for (i, key) in [
            "black", "red", "green", "yellow", "blue", "magenta", "cyan", "white",
        ]
        .iter()
        .enumerate()
        {
            let color: String = scheme.palette().ansi()[offset + i].into();
            alacritty.push_str(&format!("{key}='{color}'\n"));
        }
    }
    alacritty.push_str("[colors.cursor]\ncursor='0xABCDEF'\n");
    let converted = import_alacritty(scheme.name(), &alacritty)?;
    ensure!(
        converted == scheme,
        "Windows Terminal/Alacritty semantic mismatch"
    );
    ensure!(
        parse_terminal_fragment(&converted.to_toml_fragment()?)? == converted,
        "Alacritty fragment mismatch"
    );
    let mut ghostty =
        "foreground = FFFFFF\nbackground = #000000\ncursor-color = ABCDEF\n".to_owned();
    for (i, color) in scheme.palette().ansi().into_iter().enumerate() {
        let color: String = color.into();
        ghostty.push_str(&format!("palette = {i} = {color}\n"));
    }
    let third = import_ghostty(scheme.name(), &ghostty)?;
    ensure!(third == scheme, "Ghostty semantic mismatch");
    ensure!(
        parse_terminal_fragment(&third.to_toml_fragment()?)? == third,
        "Ghostty fragment mismatch"
    );
    let mut colors = serde_json::json!({"terminal.foreground":"#FFFFFF","terminal.background":"#000000","terminalCursor.foreground":"#ABCDEF","editor.background":"#102030","focusBorder":"#405060","editor.selectionBackground":"#01020380"});
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
    let vs_text = serde_json::json!({"name":scheme.name(),"colors":colors}).to_string();
    let vs = import_vscode(&vs_text)?;
    ensure!(*vs.terminal() == scheme, "VS Code terminal mismatch");
    ensure!(
        vs.ui_overrides().get("--bg-base") == Some(&Rgb8::new(16, 32, 48))
            && vs.ui_overrides().get("--border-strong") == Some(&Rgb8::new(64, 80, 96))
            && vs.ignored_color_count() == 1,
        "VS Code fixed UI mapping or ignored count mismatch"
    );
    ensure!(
        matches!(
            parse_terminal_fragment(&vs.to_toml_fragment()?),
            Err(ThemeError::Toml)
        ),
        "combined UI import misclassified as terminal-only"
    );
    let mut xml = String::from("<?xml version=\"1.0\"?><plist version=\"1.0\"><dict>");
    for (key, color) in [
        ("Foreground Color".to_owned(), scheme.palette().foreground),
        ("Background Color".to_owned(), scheme.palette().background),
        ("Cursor Color".to_owned(), Rgb8::new(171, 205, 239)),
    ]
    .into_iter()
    .chain(
        scheme
            .palette()
            .ansi()
            .into_iter()
            .enumerate()
            .map(|(i, c)| (format!("Ansi {i} Color"), c)),
    ) {
        let [r, g, b] = color.channels();
        xml.push_str(&format!("<key>{key}</key><dict><key>Color Space</key><string>sRGB</string><key>Red Component</key><real>{}</real><key>Green Component</key><real>{}</real><key>Blue Component</key><real>{}</real></dict>",f64::from(r)/255.0,f64::from(g)/255.0,f64::from(b)/255.0));
    }
    xml.push_str("</dict></plist>");
    let fifth = import_itermcolors(scheme.name(), &xml)?;
    ensure!(fifth == scheme, "iTerm quantized terminal mismatch");
    ensure!(
        parse_terminal_fragment(&fifth.to_toml_fragment()?)? == fifth,
        "iTerm fragment mismatch"
    );
    for (format, text, name) in [
        (ThemeImportFormat::WindowsTerminal, value.to_string(), None),
        (ThemeImportFormat::Alacritty, alacritty, Some(scheme.name())),
        (ThemeImportFormat::Ghostty, ghostty, Some(scheme.name())),
        (ThemeImportFormat::ItermXml, xml, Some(scheme.name())),
    ] {
        let imported = import_theme(format, &text, name)?;
        verify_document(&imported, emit)?;
        ensure!(
            imported.format() == format
                && *imported.terminal() == scheme
                && imported.ui_overrides().is_none(),
            "explicit terminal import mismatch"
        );
        let notes = imported.notes();
        ensure!(
            notes.ui_override_count == 0
                && notes.ignored_editor_color_count == 0
                && notes.rgb8_quantization == (format == ThemeImportFormat::ItermXml),
            "terminal conversion notes mismatch"
        );
    }
    let imported = import_theme(ThemeImportFormat::VsCode, &vs_text, None)?;
    verify_document(&imported, emit)?;
    ensure!(
        imported.ui_overrides() == Some(vs.ui_overrides())
            && *imported.terminal() == scheme
            && imported.notes().ignored_editor_color_count == 1
            && !imported.notes().rgb8_quantization,
        "combined conversion notes mismatch"
    );
    ensure!(
        imported.to_toml_fragment()? == vs.to_toml_fragment()?,
        "combined draft lost UI fields"
    );
    value["foreground"] = serde_json::json!("#777777");
    value["background"] = serde_json::json!("#ffffff");
    let low = import_windows_terminal(&value.to_string())?;
    ensure!(
        low.palette().contrast().warning == Some(ContrastWarning::LowForegroundContrast)
            && low.palette().foreground == Rgb8::new(119, 119, 119),
        "low contrast lost or colors changed"
    );
    ensure!(
        matches!(
            parse_terminal_fragment(&(fragment + "\n[ui]\nneutral='slate'\n")),
            Err(ThemeError::Toml)
        ),
        "UI document misclassified as fragment"
    );
    value["red"] = serde_json::json!("url(private-input-marker)");
    let err = import_windows_terminal(&value.to_string())
        .err()
        .ok_or_else(|| anyhow::anyhow!("CSS color admitted"))?;
    ensure!(
        err == ThemeError::Json && !err.to_string().contains("private-input-marker"),
        "parser leaked input"
    );
    for (format, system, count, mapping) in [
        (
            ThemeImportFormat::Base16Yaml,
            "base16",
            16,
            [0, 8, 11, 10, 13, 14, 12, 5, 3, 8, 11, 10, 13, 14, 12, 7],
        ),
        (
            ThemeImportFormat::Base24Yaml,
            "base24",
            24,
            [0, 8, 11, 10, 13, 14, 12, 5, 3, 18, 20, 19, 22, 23, 21, 7],
        ),
    ] {
        let mut yaml = format!(
            "system: {system}\nname: Diagnostic\nauthor: PulseTerm fixture\nvariant: dark\npalette:\n"
        );
        for i in 0..count {
            yaml.push_str(&format!("  base{i:02X}: '{i:02x}{i:02x}{i:02x}'\n"));
        }
        let r = import_theme(format, &yaml, None)?;
        ensure!(
            r.notes().base_palette_projection
                && !r.notes().rgb8_quantization
                && r.base_scheme()
                    .is_some_and(|s| s.source_palette().len() == count),
            "source base colors discarded"
        );
        ensure!(
            r.to_toml_fragment().err() == Some(ThemeError::Projection),
            "implicit fragment color loss admitted"
        );
        for (c, i) in r.terminal().palette().ansi().into_iter().zip(mapping) {
            ensure!(
                c == Rgb8::new(i, i, i),
                "official base ANSI mapping changed"
            );
        }
        ensure!(
            parse_terminal_fragment(&r.to_projected_toml_fragment()?)? == *r.terminal(),
            "explicit projection lost terminal"
        );
        verify_document(&r, emit)?;
    }
    println!(
        "PASS theme-import: Windows Terminal/Alacritty/Ghostty/VS Code/iTerm XML ANSI16 equivalence and Base16/Base24 source-preserving explicit projections, fixed VS Code UI overrides, explicit import metadata, complete light/dark TOML round trips and bounded label-free paint images, cursor mapping, terminal-only TOML and advisory WCAG; no files or UI mutation"
    );
    Ok(())
}
fn verify_document(import: &ThemeImport, emit: bool) -> Result<()> {
    for appearance in [ThemeAppearance::Light, ThemeAppearance::Dark] {
        let base = UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?);
        let theme = if import.notes().base_palette_projection {
            ensure!(
                ThemeDocument::from_import(import, appearance, base.clone()).err()
                    == Some(ThemeError::Projection),
                "implicit source color loss admitted"
            );
            ThemeDocument::from_projected_import(import, appearance, base)?
        } else {
            ThemeDocument::from_import(import, appearance, base)?
        };
        ensure!(
            theme.name() == import.terminal().name()
                && *theme.terminal() == *import.terminal().palette(),
            "complete document changed terminal"
        );
        ensure!(
            theme.appearance() == appearance
                && theme.ui().neutral() == NeutralScale::Slate
                && theme.ui().accent().value() == "cyan",
            "complete document inferred UI base"
        );
        if let Some(overrides) = import.ui_overrides() {
            for (key, color) in overrides {
                ensure!(
                    theme.ui().overrides().get(*key) == Some(color),
                    "complete document lost UI override"
                );
            }
        }
        let text = theme.to_toml()?;
        let paint = theme.paint_data()?;
        ensure!(
            paint.terminal().ansi() == &theme.terminal().ansi().map(|c| c.channels()),
            "paint changed ANSI order"
        );
        ensure!(
            paint.overrides().len() == theme.ui().overrides().len(),
            "paint lost UI slots"
        );
        ensure!(
            paint.low_foreground_contrast() == theme.terminal().contrast().warning.is_some(),
            "paint lost contrast advice"
        );
        let encoded = paint.to_json()?;
        ensure!(
            encoded.len() <= pulse_store::theme::MAX_THEME_PAINT_BYTES,
            "paint budget exceeded"
        );
        if emit {
            println!("THEME_PAINT {encoded}");
            let palette = pulse_app::terminal_palette::query_palette_from_theme(&theme);
            let query = serde_json::json!({"version":1,"palette":palette}).to_string();
            ensure!(query.len() <= 1024, "terminal palette budget exceeded");
            println!("QUERY_PALETTE {query}");
        }
        let image: serde_json::Value = serde_json::from_str(&encoded)?;
        ensure!(
            image.get("name").is_none()
                && image["terminal"]["ansi"]
                    .as_array()
                    .is_some_and(|a| a.len() == 16),
            "paint leaked labels or malformed ANSI"
        );
        ensure!(
            parse_theme_document(&text)?.paint_data()? == paint,
            "paint changed across document round trip"
        );
        ensure!(
            parse_theme_document(&text)? == theme,
            "complete document roundtrip mismatch"
        );
        ensure!(
            matches!(parse_terminal_fragment(&text), Err(ThemeError::Toml)),
            "complete document accepted as fragment"
        );
    }
    Ok(())
}
