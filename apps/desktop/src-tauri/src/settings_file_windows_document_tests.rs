use super::*;
use pulse_store::{
    settings::SettingsResolver, settings_edit::TerminalSettingEdit as Edit,
    settings_remove::TerminalSettingKey as Key,
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

#[test]
fn utf8_and_size_are_checked_before_document_editing() {
    assert!(matches!(
        document(&[0xff]),
        Err(Error::Document(SettingsError::Document))
    ));
    assert!(matches!(
        document(&vec![b'#'; SETTINGS_TEXT_LIMIT + 1]),
        Err(Error::Document(SettingsError::Size))
    ));
}
#[test]
fn malformed_unknown_future_and_invalid_known_values_never_reset() {
    for text in [
        "[terminal",
        "secret = 'must reject'",
        "schema_version = 2",
        "[terminal]\nfont_size = '14'",
        "[terminal]\nfont_size = 0",
    ] {
        assert!(document(text.as_bytes()).is_err());
        assert!(edit(text, SettingsMutation::Set(Edit::FontSize(18))).is_err());
        assert!(edit(text, SettingsMutation::Remove(Key::FontSize)).is_err());
    }
}
#[test]
fn typed_edit_preserves_other_fields_and_comments() -> TestResult {
    let original = "# retained\n[terminal]\nfont_size = 14 # keep\ncursor_blink = false\n";
    let changed = edit(original, SettingsMutation::Set(Edit::FontSize(18)))?;
    assert!(changed.contains("# retained") && changed.contains("# keep"));
    assert!(changed.contains("cursor_blink = false"));
    let resolved = SettingsResolver::resolve(Some(&changed), None, None)?;
    assert_eq!(resolved.font_size.value, 18);
    assert!(!resolved.cursor_blink.value);
    Ok(())
}
#[test]
fn explicit_false_and_remove_restore_inheritance() -> TestResult {
    let changed = edit("", SettingsMutation::Set(Edit::ConfirmClose(false)))?;
    assert!(
        !SettingsResolver::resolve(Some(&changed), None, None)?
            .confirm_close
            .value
    );
    let removed = edit(&changed, SettingsMutation::Remove(Key::ConfirmClose))?;
    assert!(
        SettingsResolver::resolve(Some(&removed), None, None)?
            .confirm_close
            .value
    );
    Ok(())
}
#[test]
fn invalid_edits_do_not_produce_a_save_document() {
    assert!(matches!(
        edit("", SettingsMutation::Set(Edit::FontSize(0))),
        Err(Error::Document(SettingsError::Value))
    ));
}
#[test]
fn missing_document_is_valid_and_empty_edit_is_a_noop() -> TestResult {
    assert_eq!(document(b"")?, "");
    assert_eq!(edit("", SettingsMutation::Remove(Key::FontSize))?, "");
    Ok(())
}
