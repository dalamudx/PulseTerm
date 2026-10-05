use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn existing_scalar_retains_every_comment_order_and_other_value() -> TestResult {
    let text = "# header\nschema_version=1 # version\n[terminal] # table\n# rationale\nfont_size = 14 # size\nfont_family='Fira Code' # family\ncursor_blink = true # blink\n";
    let actual = set_terminal_setting(text, TerminalSettingEdit::FontSize(16))?;
    assert_eq!(actual, text.replace("font_size = 14", "font_size = 16"));
    let actual = set_terminal_setting(&actual, TerminalSettingEdit::CursorBlink(false))?;
    assert_eq!(
        actual,
        text.replace("font_size = 14", "font_size = 16")
            .replace("cursor_blink = true", "cursor_blink = false")
    );
    Ok(())
}
#[test]
fn inline_table_and_dotted_keys_keep_their_spelling() -> TestResult {
    for text in [
        "terminal = { font_size = 14, cursor_blink = false } # inline\n",
        "# header\nterminal . font_size = 14 # dotted\nterminal.cursor_blink=false\n",
    ] {
        let actual = set_terminal_setting(text, TerminalSettingEdit::FontSize(17))?;
        assert_eq!(actual, text.replace("font_size = 14", "font_size = 17"));
    }
    Ok(())
}
#[test]
fn unicode_font_edits_keep_surrounding_comments() -> TestResult {
    let text = "[terminal]\n# family\nfont_family = 'Fira Code' # preserve\n";
    let actual = set_terminal_setting(
        text,
        TerminalSettingEdit::FontFamily("更纱黑体 Mono".into()),
    )?;
    assert!(actual.contains("# family\n") && actual.contains("# preserve\n"));
    assert_eq!(
        SettingsPatch::parse(&actual)?
            .terminal
            .font_family
            .as_deref(),
        Some("更纱黑体 Mono")
    );
    Ok(())
}
#[test]
fn new_scalar_never_materializes_other_defaults() -> TestResult {
    for text in [
        "",
        "# only\nschema_version = 1 # version\n",
        "terminal = {font_size=14} # inline\n",
    ] {
        let actual = set_terminal_setting(text, TerminalSettingEdit::CursorBlink(false))?;
        let patch = SettingsPatch::parse(&actual)?;
        assert_eq!(patch.terminal.cursor_blink, Some(false));
        assert!(patch.terminal.font_family.is_none());
        assert_eq!(
            patch.terminal.font_size,
            SettingsPatch::parse(text)?.terminal.font_size
        );
        if text.contains("# only") {
            assert!(actual.starts_with(text));
        }
    }
    Ok(())
}
#[test]
fn invalid_existing_or_replacement_never_gets_a_reset() {
    for text in [
        "schema_version=2",
        "secret='example'",
        "[terminal]\nfont_size=2",
        "[terminal]\nfont_size=14\nfont_size=15",
        "terminal=42",
    ] {
        assert!(set_terminal_setting(text, TerminalSettingEdit::FontSize(16)).is_err());
    }
    for edit in [
        TerminalSettingEdit::FontSize(0),
        TerminalSettingEdit::FontFamily("x; color:red".into()),
    ] {
        assert!(set_terminal_setting("", edit).is_err());
    }
}
#[test]
fn new_dotted_value_keeps_all_existing_bytes() -> TestResult {
    let text = "# header\nterminal . font_size = 14 # one\nterminal.font_family='Fira Code' # two";
    let actual = set_terminal_setting(text, TerminalSettingEdit::CursorBlink(false))?;
    assert_eq!(
        actual,
        text.to_owned() + "\nterminal.cursor_blink = false\n"
    );
    Ok(())
}
#[test]
fn escaped_unicode_token_is_replaced_without_touching_surroundings() -> TestResult {
    let text = "[terminal]\nfont_family = \"更\\u7eb1黑体\" # keep\nfont_size=14\n";
    let actual = set_terminal_setting(text, TerminalSettingEdit::FontFamily("monospace".into()))?;
    assert_eq!(
        actual,
        "[terminal]\nfont_family = \"monospace\" # keep\nfont_size=14\n"
    );
    Ok(())
}
#[test]
fn input_and_result_stay_under_the_document_budget() {
    let text = "#".repeat(crate::settings::SETTINGS_TEXT_LIMIT);
    assert!(matches!(
        set_terminal_setting(&text, TerminalSettingEdit::FontSize(14)),
        Err(SettingsError::Size)
    ));
    assert!(matches!(
        set_terminal_setting(&(text + "#"), TerminalSettingEdit::FontSize(14)),
        Err(SettingsError::Size)
    ));
}
