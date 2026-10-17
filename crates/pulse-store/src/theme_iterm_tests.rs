use super::*;
use crate::theme::{MAX_SCHEME_BYTES, parse_terminal_fragment};
fn entry(key: &str, r: &str, g: &str, b: &str) -> String {
    format!(
        "<key>{key}</key><dict><key>Color Space</key><string>sRGB</string><key>Red Component</key><real>{r}</real><key>Green Component</key><real>{g}</real><key>Blue Component</key><real>{b}</real></dict>"
    )
}
fn fixture() -> String {
    let mut t =
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?><plist version=\"1.0\"><dict>".to_owned();
    t.push_str(&entry("Foreground Color", "1", "1", "1"));
    t.push_str(&entry("Background Color", "0", "0", "0"));
    for i in 0..16 {
        t.push_str(&entry(
            &format!("Ansi {i} Color"),
            "0",
            "0",
            &(f64::from(i) / 255.0).to_string(),
        ));
    }
    t.push_str("</dict></plist>");
    t
}
#[test]
fn all_slots_and_quantization_roundtrip() -> Result<(), Box<dyn std::error::Error>> {
    let t = import_itermcolors("中文 XML", &fixture())?;
    for (i, c) in t.palette().ansi().iter().enumerate() {
        assert_eq!(usize::from(c.channels()[2]), i);
    }
    assert!(parse_terminal_fragment(&t.to_toml_fragment()?)? == t);
    let t = import_itermcolors(
        "Fixture",
        &fixture().replace("<real>1</real>", "<real>0.5</real>"),
    )?;
    assert_eq!(t.palette().foreground, Rgb8::new(128, 128, 128));
    Ok(())
}
#[test]
fn optional_cursor_selection_are_not_invented() -> Result<(), Box<dyn std::error::Error>> {
    let t = fixture();
    let mut extra = String::new();
    for k in [
        "Cursor Color",
        "Cursor Text Color",
        "Selection Color",
        "Selected Text Color",
    ] {
        extra.push_str(&entry(k, "1", "0", "0"));
    }
    let s = import_itermcolors(
        "Fixture",
        &t.replace("</dict></plist>", &format!("{extra}</dict></plist>")),
    )?;
    for c in [
        s.palette().cursor,
        s.palette().cursor_text,
        s.palette().selection_background,
        s.palette().selection_foreground,
    ] {
        assert_eq!(c, Some(Rgb8::new(255, 0, 0)));
    }
    assert_eq!(import_itermcolors("Fixture", &t)?.palette().cursor, None);
    Ok(())
}
#[test]
fn canonical_doctype_comments_and_numeric_entities_are_inert()
-> Result<(), Box<dyn std::error::Error>> {
    let t = fixture()
        .replace(
            "<plist",
            &format!("<!DOCTYPE {}><!-- marker --><plist", DTD),
        )
        .replace("<string>sRGB</string>", "<string>sR&#71;B</string>");
    assert!(import_itermcolors("Fixture", &t).is_ok());
    Ok(())
}
#[test]
fn duplicate_color_and_component_keys_reject() {
    let t = fixture();
    for extra in [
        entry("Foreground Color", "1", "1", "1"),
        entry("Ansi 5 Color", "0", "0", "0"),
    ] {
        assert_eq!(
            import_itermcolors(
                "Fixture",
                &t.replace("</dict></plist>", &format!("{extra}</dict></plist>"))
            )
            .err(),
            Some(ThemeError::Iterm)
        );
    }
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &t.replacen(
                "<key>Red Component</key>",
                "<key>Red Component</key><real>0</real><key>Red Component</key>",
                1
            )
        )
        .err(),
        Some(ThemeError::Iterm)
    );
}
#[test]
fn missing_required_and_unknown_colors_reject() {
    let t = fixture();
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &t.replace(&entry("Foreground Color", "1", "1", "1"), "")
        )
        .err(),
        Some(ThemeError::Iterm)
    );
    let extra = entry("Bold Color", "1", "1", "1");
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &t.replace("</dict></plist>", &format!("{extra}</dict></plist>"))
        )
        .err(),
        Some(ThemeError::Iterm)
    );
}
#[test]
fn nonfinite_range_color_space_and_alpha_reject() {
    let t = fixture();
    for value in ["NaN", "inf", "-0.01", "1.01"] {
        assert_eq!(
            import_itermcolors(
                "Fixture",
                &t.replacen("<real>1</real>", &format!("<real>{value}</real>"), 1)
            )
            .err(),
            Some(ThemeError::Iterm)
        );
    }
    for space in ["Generic RGB", "Display P3", "sRGB&unknown;", ""] {
        assert_eq!(
            import_itermcolors(
                "Fixture",
                &t.replace(
                    "<string>sRGB</string>",
                    &format!("<string>{space}</string>")
                )
            )
            .err(),
            Some(ThemeError::Iterm)
        );
    }
    for alpha in ["0.5", "NaN"] {
        assert_eq!(
            import_itermcolors(
                "Fixture",
                &t.replacen(
                    "<key>Color Space</key>",
                    &format!(
                        "<key>Alpha Component</key><real>{alpha}</real><key>Color Space</key>"
                    ),
                    1
                )
            )
            .err(),
            Some(ThemeError::Iterm)
        );
    }
}
#[test]
fn external_entity_binary_trailing_root_and_deep_documents_reject() {
    let t = fixture();
    for bad in [
        t.replace(
            "<plist",
            "<!DOCTYPE plist SYSTEM \"file:///private-input-marker\"><plist",
        ),
        t.replace(
            "<plist",
            "<!DOCTYPE plist [<!ENTITY x SYSTEM \"https://untrusted.invalid\">]><plist",
        ),
        format!("{t}{t}"),
        "bplist00private-input-marker".to_owned(),
        t.replace(
            "<real>1</real>",
            "<dict><dict><string>1</string></dict></dict>",
        ),
        t.replace("<string>sRGB</string>", "<string><![CDATA[sRGB]]></string>"),
    ] {
        let e = import_itermcolors("Fixture", &bad).err();
        assert_eq!(e, Some(ThemeError::Iterm));
        assert!(
            !e.map(|e| e.to_string())
                .unwrap_or_default()
                .contains("private-input-marker")
        );
    }
}
#[test]
fn budgets_and_alpha_one_are_explicit() -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(
        import_itermcolors("Fixture", &" ".repeat(MAX_SCHEME_BYTES + 1)).err(),
        Some(ThemeError::Budget)
    );
    assert_eq!(
        import_itermcolors("bad\0name", &fixture()).err(),
        Some(ThemeError::Name)
    );
    let t = fixture().replacen(
        "<key>Color Space</key>",
        "<key>Alpha Component</key><real>1</real><key>Color Space</key>",
        1,
    );
    assert!(import_itermcolors("Fixture", &t).is_ok());
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &fixture().replace("<key>Color Space</key><string>sRGB</string>", "")
        )
        .err(),
        Some(ThemeError::Iterm)
    );
    Ok(())
}
#[test]
fn event_quota_escaped_duplicates_and_xml_encoding_reject() {
    let t = fixture();
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &t.replace("<plist", &format!("{}<plist", "<!--x-->".repeat(2048)))
        )
        .err(),
        Some(ThemeError::Iterm)
    );
    let repeated = entry("Ansi &#53; Color", "0", "0", "0");
    assert_eq!(
        import_itermcolors(
            "Fixture",
            &t.replace("</dict></plist>", &format!("{repeated}</dict></plist>"))
        )
        .err(),
        Some(ThemeError::Iterm)
    );
    for bad in [
        t.replace("UTF-8", "UTF-16"),
        t.replace("<string>sRGB</string>", "<string>sRGB&bogus;</string>"),
        t.replace("<real>1</real>", "<data>AAAA</data>"),
        t.clone() + "nonxml trailing text",
    ] {
        assert_eq!(
            import_itermcolors("Fixture", &bad).err(),
            Some(ThemeError::Iterm)
        );
    }
}
