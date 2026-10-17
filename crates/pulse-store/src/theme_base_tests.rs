use super::*;
fn fixture(system: BaseSystem, legacy: bool) -> String {
    let mut s = if legacy {
        "scheme: 'Literal 中文 /../name'\nauthor: Tester\n".into()
    } else {
        format!(
            "system: {}\nname: 'Literal 中文 /../name'\nauthor: Tester\nvariant: dark\npalette:\n",
            system.name()
        )
    };
    for i in 0..system.count() {
        s.push_str(&format!(
            "{}base{i:02X}: '{i:02x}{i:02x}{i:02x}'\n",
            if legacy { "" } else { "  " }
        ));
    }
    s
}
#[test]
fn current_and_legacy_base16_equal_source_and_projection() -> Result<(), Box<dyn std::error::Error>>
{
    let a = import_base_scheme(&fixture(BaseSystem::Base16, false), BaseSystem::Base16)?;
    let b = import_base_scheme(&fixture(BaseSystem::Base16, true), BaseSystem::Base16)?;
    assert_eq!(a.source_palette(), b.source_palette());
    assert!(a.terminal() == b.terminal());
    assert_eq!(a.variant(), Some(ThemeAppearance::Dark));
    assert_eq!(b.variant(), None);
    assert_eq!(
        a.metadata().get("author").map(String::as_str),
        Some("Tester")
    );
    let indices = [0, 8, 11, 10, 13, 14, 12, 5, 3, 8, 11, 10, 13, 14, 12, 7];
    for (c, i) in a.terminal().palette().ansi().into_iter().zip(indices) {
        assert_eq!(c, a.source_palette()[i]);
    }
    Ok(())
}
#[test]
fn base24_keeps_all_bases_and_uses_real_bright_palette() -> Result<(), Box<dyn std::error::Error>> {
    let a = import_base_scheme(&fixture(BaseSystem::Base24, false), BaseSystem::Base24)?;
    assert_eq!(a.source_palette().len(), 24);
    let indices = [0, 8, 11, 10, 13, 14, 12, 5, 3, 18, 20, 19, 22, 23, 21, 7];
    for (c, i) in a.terminal().palette().ansi().into_iter().zip(indices) {
        assert_eq!(c, a.source_palette()[i]);
    }
    assert_eq!(a.source_palette()[16], Rgb8::new(16, 16, 16));
    assert!(a.terminal().palette().ansi()[1] != a.terminal().palette().ansi()[9]);
    Ok(())
}
#[test]
fn explicit_system_missing_or_extra_slots_and_mixed_layout_reject() {
    let s = fixture(BaseSystem::Base16, false);
    for bad in [
        s.replace("base16", "base24"),
        s.replace("  base0F: '0f0f0f'\n", ""),
        s.clone() + "  base10: '101010'\n",
        s.clone() + "base00: '000000'\n",
        s.replace("name:", "scheme:"),
        s.replace("author: Tester\n", ""),
        s.replace("dark", "auto"),
    ] {
        assert_eq!(
            import_base_scheme(&bad, BaseSystem::Base16).err(),
            Some(ThemeError::BaseYaml)
        );
    }
}
#[test]
fn duplicate_keys_including_decoded_escape_reject() {
    let s = fixture(BaseSystem::Base16, false);
    for bad in [
        s.clone() + "  base00: '000000'\n",
        s.clone() + "  \"base0\\u0030\": '000000'\n",
        s.replace("name:", "author: Other\nname:"),
        s.replace("palette:", "palette: {}\npalette:"),
    ] {
        assert_eq!(
            import_base_scheme(&bad, BaseSystem::Base16).err(),
            Some(ThemeError::BaseYaml)
        );
    }
}
#[test]
fn aliases_tags_merge_sequences_and_nested_metadata_reject() {
    let s = fixture(BaseSystem::Base16, false);
    for bad in [
        s.replace("'000000'", "&x '000000'")
            .replace("'010101'", "*x"),
        s.replace("'000000'", "!!str '000000'"),
        s.replace("author: Tester", "author: [Tester]"),
        s.replace("author: Tester", "author:\n  key: nested"),
        s.clone() + "  <<: {base00: '000000'}\n",
        s.replace("palette:", "palette: &p"),
    ] {
        assert_eq!(
            import_base_scheme(&bad, BaseSystem::Base16).err(),
            Some(ThemeError::BaseYaml)
        );
    }
}
#[test]
fn extra_documents_trailing_content_empty_and_invalid_yaml_reject() {
    let s = fixture(BaseSystem::Base16, false);
    for bad in [
        s.clone() + "---\n{}\n",
        s + "\n...\nprivate-secret\n",
        "".into(),
        "null".into(),
        "name: [".into(),
    ] {
        assert!(import_base_scheme(&bad, BaseSystem::Base16).is_err());
    }
}
#[test]
fn hex_forms_canonical_key_case_and_alpha_reject() -> Result<(), Box<dyn std::error::Error>> {
    let s = fixture(BaseSystem::Base16, false);
    let a = import_base_scheme(&s.replace("'0a0a0a'", "'#0A0a0A'"), BaseSystem::Base16)?;
    assert_eq!(a.source_palette()[10], Rgb8::new(10, 10, 10));
    for bad in [
        s.replace("base0A", "base0a"),
        s.replace("'000000'", "'#000'"),
        s.replace("'000000'", "'#00000080'"),
    ] {
        assert!(import_base_scheme(&bad, BaseSystem::Base16).is_err());
    }
    Ok(())
}
#[test]
fn explicit_projection_required_and_original_bases_remain() -> Result<(), Box<dyn std::error::Error>>
{
    use super::super::{
        AccentChoice, NeutralScale, ThemeDocument, ThemeImportFormat, UiThemeBase, import_theme,
        parse_terminal_fragment, parse_theme_document,
    };
    for (system, format) in [
        (BaseSystem::Base16, ThemeImportFormat::Base16Yaml),
        (BaseSystem::Base24, ThemeImportFormat::Base24Yaml),
    ] {
        let text = fixture(system, false);
        let r = import_theme(format, &text, None)?;
        assert!(r.notes().base_palette_projection);
        assert!(!r.notes().rgb8_quantization);
        assert!(r.ui_overrides().is_none());
        assert_eq!(
            r.base_scheme()
                .ok_or("missing original bases")?
                .source_palette()
                .len(),
            system.count()
        );
        assert_eq!(r.to_toml_fragment().err(), Some(ThemeError::Projection));
        assert_eq!(
            import_theme(format, &text, Some("replacement")).err(),
            Some(ThemeError::NamePolicy)
        );
        let ui = UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?);
        assert_eq!(
            ThemeDocument::from_import(&r, ThemeAppearance::Dark, ui.clone()).err(),
            Some(ThemeError::Projection)
        );
        let doc = ThemeDocument::from_projected_import(&r, ThemeAppearance::Dark, ui)?;
        assert!(parse_theme_document(&doc.to_toml()?)? == doc);
        assert!(parse_terminal_fragment(&r.to_projected_toml_fragment()?)? == *r.terminal());
        assert_eq!(
            r.base_scheme()
                .ok_or("lost original bases")?
                .source_palette()
                .len(),
            system.count()
        );
    }
    Ok(())
}
#[test]
fn generated_color_samples_preserve_source_and_ansi_mapping()
-> Result<(), Box<dyn std::error::Error>> {
    for seed in 0..128 {
        for system in [BaseSystem::Base16, BaseSystem::Base24] {
            let mut text = fixture(system, false);
            for i in 0..system.count() {
                let color = format!(
                    "{:02x}{:02x}{:02x}",
                    (i + seed) % 256,
                    (i * 7 + seed) % 256,
                    (i * 13 + seed) % 256
                );
                text = text.replace(&format!("'{i:02x}{i:02x}{i:02x}'"), &format!("'#{color}'"));
            }
            let r = import_base_scheme(&text, system)?;
            for (i, color) in r.source_palette().iter().enumerate() {
                assert_eq!(
                    *color,
                    Rgb8::new(
                        ((i + seed) % 256) as u8,
                        ((i * 7 + seed) % 256) as u8,
                        ((i * 13 + seed) % 256) as u8
                    )
                );
            }
        }
    }
    Ok(())
}

#[test]
fn errors_no_content_and_fixed_byte_budget() -> Result<(), Box<dyn std::error::Error>> {
    let error = import_base_scheme("private-secret: x", BaseSystem::Base16)
        .err()
        .ok_or("invalid input accepted")?;
    assert!(!error.to_string().contains("private-secret"));
    assert_eq!(
        import_base_scheme(
            &" ".repeat(super::super::MAX_SCHEME_BYTES + 1),
            BaseSystem::Base16
        )
        .err(),
        Some(ThemeError::Budget)
    );
    Ok(())
}
