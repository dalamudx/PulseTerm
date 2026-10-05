use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn compiled_defaults_are_complete_and_not_user_origin() -> TestResult {
    let value = SettingsResolver::resolve(None, None, None)?;
    assert_eq!(value.font_size.value, 14);
    assert_eq!(value.font_family.value, "monospace");
    assert!(!value.cursor_blink.value);
    assert_eq!(value.font_size.source, SettingsSource::Default);
    assert_eq!(value.font_family.source, SettingsSource::Default);
    assert_eq!(value.cursor_blink.source, SettingsSource::Default);
    Ok(())
}
#[test]
fn each_field_keeps_its_own_highest_explicit_layer() -> TestResult {
    let value = SettingsResolver::resolve(
        Some("[terminal]\nfont_size=18\nfont_family='Sarasa Mono SC'"),
        Some("[terminal]\nfont_size=22\ncursor_blink=true"),
        Some("[terminal]\ncursor_blink=false"),
    )?;
    assert_eq!(
        value.font_size,
        Setting {
            value: 22,
            source: SettingsSource::Group
        }
    );
    assert_eq!(
        value.font_family,
        Setting {
            value: "Sarasa Mono SC".into(),
            source: SettingsSource::User
        }
    );
    assert_eq!(
        value.cursor_blink,
        Setting {
            value: false,
            source: SettingsSource::Host
        }
    );
    Ok(())
}
#[test]
fn explicit_value_equal_to_default_still_has_user_provenance() -> TestResult {
    let value = SettingsResolver::resolve(
        Some("[terminal]\nfont_size=14\ncursor_blink=false"),
        Some(""),
        Some("[terminal]"),
    )?;
    assert_eq!(value.font_size.source, SettingsSource::User);
    assert_eq!(value.cursor_blink.source, SettingsSource::User);
    Ok(())
}
#[test]
fn malformed_unknown_duplicate_or_future_documents_fail_closed() {
    for text in [
        "password='sensitive-fixture'",
        "schema_version=2",
        "schema_version=0",
        "[terminal]\nunknown=1",
        "[terminal]\nfont_size=12\nfont_size=14",
        "[terminal]\nfont_size='14'",
        "[terminal]\nfont_size=0",
        "[terminal]\nfont_size=73",
        "[terminal]\ncursor_blink='false'",
        "[terminal]\nfont_size=-1",
        "[terminal]\nfont_family=[]",
    ] {
        assert!(SettingsPatch::parse(text).is_err(), "fixture accepted");
    }
    assert!(
        SettingsResolver::resolve(
            Some("[terminal]\nfont_size=0"),
            None,
            Some("[terminal]\nfont_size=14")
        )
        .is_err()
    );
}
#[test]
fn text_budget_is_checked_before_toml_parsing() {
    let at_limit = format!("#{}", "x".repeat(SETTINGS_TEXT_LIMIT - 1));
    assert!(SettingsPatch::parse(&at_limit).is_ok());
    assert_eq!(
        SettingsPatch::parse(&(at_limit + "x")),
        Err(SettingsError::Size)
    );
}
#[test]
fn safe_names_support_unicode_but_never_css_grammar() -> TestResult {
    for name in ["monospace", "Noto Sans Mono", "微软雅黑", "Sarasa-Mono_SC"] {
        SettingsPatch::parse(&format!("[terminal]\nfont_family='{name}'"))?;
    }
    for name in [
        "",
        " ",
        " mono",
        "mono ",
        "mono, serif",
        "url(https://invalid)",
        "mono; color:red",
        "mono{}",
        "<style>",
        "a\nb",
        "a\\b",
    ] {
        let patch = SettingsPatch {
            terminal: TerminalPatch {
                font_family: Some(name.into()),
                ..Default::default()
            },
            ..Default::default()
        };
        assert_eq!(patch.validate(), Err(SettingsError::Value));
    }
    let mut patch = SettingsPatch::default();
    patch.terminal.font_family = Some("汉".repeat(128));
    patch.validate()?;
    patch.terminal.font_family = Some("汉".repeat(129));
    assert_eq!(patch.validate(), Err(SettingsError::Value));
    Ok(())
}
#[test]
fn schema_includes_the_same_bounds_and_rejects_unknown_properties() -> TestResult {
    let schema = serde_json::to_value(SettingsResolver::schema())?;
    assert_eq!(schema["additionalProperties"], false);
    let terminal = &schema["$defs"]["TerminalPatch"];
    assert_eq!(terminal["additionalProperties"], false);
    assert_eq!(terminal["properties"]["font_size"]["minimum"], 6);
    assert_eq!(terminal["properties"]["font_size"]["maximum"], 72);
    assert_eq!(
        terminal["properties"]["font_family"]["pattern"],
        FONT_NAME_PATTERN
    );
    assert_eq!(terminal["properties"]["font_family"]["maxLength"], 128);
    Ok(())
}
#[test]
fn parse_failures_do_not_echo_sensitive_or_invalid_content() {
    let text = "password='sensitive-fixture'";
    if let Err(error) = SettingsPatch::parse(text) {
        assert!(!format!("{error:?} {error}").contains("sensitive-fixture"));
    } else {
        panic!("unexpected acceptance");
    }
}
