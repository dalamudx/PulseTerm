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
fn selection_copy_defaults_false_and_explicit_false_overrides_all_lower_layers() -> TestResult {
    let initial = SettingsResolver::resolve(None, None, None)?;
    assert!(!initial.copy_on_select.value);
    assert_eq!(initial.copy_on_select.source, SettingsSource::Default);
    let enabled = "terminal.copy_on_select=true";
    let disabled = "terminal.copy_on_select=false";
    assert!(
        SettingsResolver::resolve(Some(enabled), None, None)?
            .copy_on_select
            .value
    );
    let resolved = SettingsResolver::resolve(Some(enabled), Some(disabled), Some(enabled))?;
    assert!(resolved.copy_on_select.value);
    assert_eq!(resolved.copy_on_select.source, SettingsSource::Host);
    let resolved = SettingsResolver::resolve(Some(enabled), Some(enabled), Some(disabled))?;
    assert!(!resolved.copy_on_select.value);
    assert_eq!(resolved.copy_on_select.source, SettingsSource::Host);
    assert!(SettingsPatch::parse("terminal.copy_on_select='true'").is_err());
    Ok(())
}

#[test]
fn right_click_paste_default_and_host_false_override() -> Result<(), SettingsError> {
    let resolved = SettingsResolver::resolve(None, None, None)?;
    assert!(!resolved.right_click_paste.value);
    assert_eq!(resolved.right_click_paste.source, SettingsSource::Default);
    let enabled = "terminal.right_click_paste=true";
    let disabled = "terminal.right_click_paste=false";
    let resolved = SettingsResolver::resolve(Some(enabled), Some(enabled), Some(disabled))?;
    assert!(!resolved.right_click_paste.value);
    assert_eq!(resolved.right_click_paste.source, SettingsSource::Host);
    assert!(SettingsPatch::parse("terminal.right_click_paste='true'").is_err());
    Ok(())
}

#[test]
fn confirm_close_defaults_true_and_explicit_false_overrides_all_lower_layers() -> TestResult {
    let initial = SettingsResolver::resolve(None, None, None)?;
    assert!(initial.confirm_close.value);
    assert_eq!(initial.confirm_close.source, SettingsSource::Default);
    let enabled = "terminal.confirm_close=true";
    let disabled = "terminal.confirm_close=false";
    assert!(
        !SettingsResolver::resolve(Some(disabled), None, None)?
            .confirm_close
            .value
    );
    let resolved = SettingsResolver::resolve(Some(disabled), Some(enabled), Some(disabled))?;
    assert!(!resolved.confirm_close.value);
    assert_eq!(resolved.confirm_close.source, SettingsSource::Host);
    let resolved = SettingsResolver::resolve(Some(disabled), Some(disabled), Some(enabled))?;
    assert!(resolved.confirm_close.value);
    assert_eq!(resolved.confirm_close.source, SettingsSource::Host);
    assert!(SettingsPatch::parse("terminal.confirm_close='true'").is_err());
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

#[test]
fn cell_metrics_keep_independent_sources_and_explicit_zero() -> TestResult {
    let default = SettingsResolver::resolve(None, None, None)?;
    assert_eq!(default.font_size.value, 14);
    assert_eq!(default.line_height_percent.value, 100);
    assert_eq!(default.letter_spacing.value, 0);
    let value = SettingsResolver::resolve(
        Some("[terminal]\nline_height_percent=150\nletter_spacing=2"),
        Some("[terminal]\nline_height_percent=200"),
        Some("[terminal]\nletter_spacing=0"),
    )?;
    assert_eq!(
        value.line_height_percent,
        Setting {
            value: 200,
            source: SettingsSource::Group
        }
    );
    assert_eq!(
        value.letter_spacing,
        Setting {
            value: 0,
            source: SettingsSource::Host
        }
    );
    Ok(())
}
#[test]
fn cell_metrics_scalar_edits_preserve_other_content_and_reset_inheritance() -> TestResult {
    use crate::settings_edit::{TerminalSettingEdit as Edit, set_terminal_setting};
    use crate::settings_remove::{TerminalSettingKey as Key, remove_terminal_setting};
    let text = "# retained\n[terminal]\nfont_size=18 # size\nletter_spacing=2 # cell width\ncursor_blink=false\n";
    let changed = set_terminal_setting(text, Edit::LineHeightPercent(150))?;
    let changed = set_terminal_setting(&changed, Edit::LetterSpacing(0))?;
    assert!(
        changed.contains("# retained")
            && changed.contains("font_size=18 # size")
            && changed.contains("# cell width")
    );
    assert_eq!(
        SettingsResolver::resolve(Some(&changed), None, None)?
            .letter_spacing
            .source,
        SettingsSource::User
    );
    let reset = remove_terminal_setting(&changed, Key::LetterSpacing)?;
    let reset = remove_terminal_setting(&reset, Key::LineHeightPercent)?;
    let resolved = SettingsResolver::resolve(Some(&reset), None, None)?;
    assert_eq!(resolved.line_height_percent.source, SettingsSource::Default);
    assert_eq!(resolved.letter_spacing.source, SettingsSource::Default);
    assert_eq!(resolved.font_size.value, 18);
    assert!(set_terminal_setting(text, Edit::LineHeightPercent(99)).is_err());
    assert!(set_terminal_setting(text, Edit::LetterSpacing(9)).is_err());
    Ok(())
}
#[test]
fn cell_metrics_reject_unsafe_values_and_match_generated_schema() -> TestResult {
    for text in [
        "terminal.line_height_percent=99",
        "terminal.line_height_percent=301",
        "terminal.line_height_percent=150.5",
        "terminal.letter_spacing=-1",
        "terminal.letter_spacing=9",
        "terminal.letter_spacing=0.5",
        "terminal.letter_spacing='0'",
        "terminal.line_height_percent=true",
    ] {
        assert!(SettingsPatch::parse(text).is_err());
    }
    let schema = serde_json::to_value(SettingsResolver::schema())?;
    let properties = &schema["$defs"]["TerminalPatch"]["properties"];
    assert_eq!(properties["line_height_percent"]["minimum"], 100);
    assert_eq!(properties["line_height_percent"]["maximum"], 300);
    assert_eq!(properties["letter_spacing"]["minimum"], 0);
    assert_eq!(properties["letter_spacing"]["maximum"], 8);
    Ok(())
}

#[test]
fn font_preferences_keep_independent_sources_and_safe_names()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    use crate::settings_remove::{TerminalSettingKey, remove_terminal_setting};
    let result = SettingsResolver::resolve(
        Some(
            "terminal.font_family='JetBrains Mono'\nterminal.font_fallback='Droid Sans Fallback'\nterminal.font_ligatures=true",
        ),
        Some("terminal.font_ligatures=false"),
        Some("terminal.font_fallback='WenQuanYi Zen Hei Mono'"),
    )?;
    assert_eq!(result.font_size.value, 14);
    assert_eq!(result.font_fallback.source, SettingsSource::Host);
    assert_eq!(result.font_ligatures.source, SettingsSource::Group);
    assert!(!result.font_ligatures.value);
    assert_eq!(result.font_family.source, SettingsSource::User);
    for text in [
        "terminal.font_fallback='mono,serif'",
        "terminal.font_fallback='url(x)'",
        "terminal.font_fallback='mono; color:red'",
        "terminal.font_fallback=''",
        "terminal.font_ligatures=1",
    ] {
        assert!(SettingsPatch::parse(text).is_err());
    }
    let text = "schema_version=1\n[terminal]\nfont_size = 14 # size untouched\nfont_fallback = 'monospace' # family comment\nfont_ligatures = false # explicit false\n";
    let changed = set_terminal_setting(
        text,
        TerminalSettingEdit::FontFallback("Droid Sans Fallback".into()),
    )?;
    assert!(changed.contains("font_size = 14 # size untouched"));
    assert!(changed.contains("font_ligatures = false # explicit false"));
    let removed = remove_terminal_setting(&changed, TerminalSettingKey::FontFallback)?;
    assert!(removed.contains("# family comment"));
    let patch = SettingsPatch::parse(&removed)?;
    assert!(patch.terminal.font_fallback.is_none());
    assert_eq!(patch.terminal.font_ligatures, Some(false));
    assert_eq!(
        SettingsResolver::resolve(Some(&removed), None, None)?
            .font_fallback
            .value,
        "monospace"
    );
    Ok(())
}

#[test]
fn ui_appearance_defaults_user_origin_and_terminal_independence() -> TestResult {
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    assert_eq!(
        SettingsResolver::resolve(None, None, None)?
            .ui_appearance
            .value,
        UiAppearance::Light
    );
    for mode in [
        UiAppearance::Light,
        UiAppearance::Dark,
        UiAppearance::System,
    ] {
        let text = set_terminal_setting(
            "# preferences\n[terminal]\nfont_size=19\n",
            TerminalSettingEdit::UiAppearance(mode),
        )?;
        let value = SettingsResolver::resolve(Some(&text), None, None)?;
        assert_eq!(
            value.ui_appearance,
            Setting {
                value: mode,
                source: SettingsSource::User
            }
        );
        assert_eq!(value.font_size.value, 19);
    }
    Ok(())
}
#[test]
fn ui_appearance_rejects_unknown_values_and_non_user_overlays() -> TestResult {
    for text in [
        "[ui]\nappearance='auto'",
        "[ui]\nappearance=1",
        "[ui]\nappearance='dark'\ncss='color:red'",
    ] {
        assert!(SettingsPatch::parse(text).is_err());
    }
    let text = "[ui]\nappearance='dark'";
    assert_eq!(
        SettingsResolver::resolve(None, Some(text), None),
        Err(SettingsError::Value)
    );
    assert_eq!(
        SettingsResolver::resolve(None, None, Some(text)),
        Err(SettingsError::Value)
    );
    Ok(())
}
#[test]
fn ui_appearance_edits_preserve_comments_dotted_keys_and_terminal_values() -> TestResult {
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    use crate::settings_remove::{TerminalSettingKey, remove_terminal_setting};
    for text in [
        "# keep\n[ui] # display\nappearance = 'light' # choice\n[terminal]\nfont_size=19\n",
        "ui.appearance = 'light' # choice\nterminal.font_size=19\n",
        "ui = { appearance = 'light' } # choice\n[terminal]\nfont_size=19\n",
    ] {
        let changed =
            set_terminal_setting(text, TerminalSettingEdit::UiAppearance(UiAppearance::Dark))?;
        assert_eq!(changed, text.replace("'light'", "\"dark\""));
        let removed = remove_terminal_setting(&changed, TerminalSettingKey::UiAppearance)?;
        let value = SettingsResolver::resolve(Some(&removed), None, None)?;
        assert_eq!(value.ui_appearance.source, SettingsSource::Default);
        assert_eq!(value.ui_appearance.value, UiAppearance::Light);
        assert_eq!(value.font_size.value, 19);
        assert!(removed.contains("# choice"));
        assert_eq!(
            remove_terminal_setting(&removed, TerminalSettingKey::UiAppearance)?,
            removed
        );
    }
    Ok(())
}
#[test]
fn ui_appearance_insert_into_empty_inline_and_dotted_tables() -> TestResult {
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    for text in ["", "ui = {}\n", "[ui]\n", "[terminal]\nfont_size=19\n"] {
        let changed = set_terminal_setting(
            text,
            TerminalSettingEdit::UiAppearance(UiAppearance::System),
        )?;
        assert_eq!(
            SettingsPatch::parse(&changed)?.ui.appearance,
            Some(UiAppearance::System)
        );
    }
    Ok(())
}

#[test]
fn history_capacity_settings_validate_inherit_save_and_reset() -> TestResult {
    use crate::{
        settings_edit::{self, TerminalSettingEdit},
        settings_remove::{self, TerminalSettingKey},
    };
    let default = SettingsResolver::resolve(None, None, None)?;
    assert_eq!(default.history_limit.value, 100_000);
    let text = "# retained comment\n[terminal]\nfont_size=14\n";
    let changed =
        settings_edit::set_terminal_setting(text, TerminalSettingEdit::HistoryLimit(3000))?;
    assert!(changed.contains("# retained comment"));
    assert!(changed.contains("font_size=14"));
    let resolved = SettingsResolver::resolve(Some(&changed), None, None)?;
    assert_eq!(resolved.terminal_startup(false).history_limit, 3000);
    assert_eq!(resolved.history_limit.source, SettingsSource::User);
    assert_eq!(
        SettingsResolver::resolve(Some(&changed), Some("[terminal]\nhistory_limit=2000"), None)?
            .history_limit
            .value,
        2000
    );
    let reset =
        settings_remove::remove_terminal_setting(&changed, TerminalSettingKey::HistoryLimit)?;
    assert_eq!(
        SettingsResolver::resolve(Some(&reset), None, None)?
            .history_limit
            .value,
        100_000
    );
    for value in [0, 999, 100_001, u32::MAX] {
        assert!(
            settings_edit::set_terminal_setting(text, TerminalSettingEdit::HistoryLimit(value))
                .is_err()
        );
    }
    Ok(())
}

#[test]
fn scroll_input_setting_false_inherits_and_round_trips() -> TestResult {
    use crate::{
        settings_edit::{TerminalSettingEdit, set_terminal_setting},
        settings_remove::{TerminalSettingKey, remove_terminal_setting},
    };
    assert!(
        SettingsResolver::resolve(None, None, None)?
            .scroll_on_input
            .value
    );
    let original = "# preserve\n[terminal]\nfont_size=18\n";
    let changed = set_terminal_setting(original, TerminalSettingEdit::ScrollOnInput(false))?;
    let user = SettingsResolver::resolve(Some(&changed), None, None)?;
    assert!(!user.scroll_on_input.value);
    assert_eq!(user.scroll_on_input.source, SettingsSource::User);
    assert_eq!(user.font_size.value, 18);
    let host = SettingsResolver::resolve(
        Some(&changed),
        Some("[terminal]\nscroll_on_input=true"),
        Some("[terminal]\nscroll_on_input=false"),
    )?;
    assert!(!host.scroll_on_input.value);
    assert_eq!(host.scroll_on_input.source, SettingsSource::Host);
    let reset = remove_terminal_setting(&changed, TerminalSettingKey::ScrollOnInput)?;
    assert_eq!(reset.trim_end(), original.trim_end());
    assert!(
        SettingsResolver::resolve(Some(&reset), None, None)?
            .scroll_on_input
            .value
    );
    for value in ["0", "\"false\"", "[]"] {
        assert!(SettingsPatch::parse(&format!("[terminal]\nscroll_on_input={value}")).is_err());
    }
    Ok(())
}

#[test]
fn scroll_output_setting_round_trip_and_inheritance() -> TestResult {
    use crate::{
        settings_edit::{TerminalSettingEdit, set_terminal_setting},
        settings_remove::{TerminalSettingKey, remove_terminal_setting},
    };
    let original = "# preserve\n[terminal]\nfont_size=18\nscroll_on_input=false\n";
    assert!(
        !SettingsResolver::resolve(None, None, None)?
            .scroll_on_output
            .value
    );
    let enabled = set_terminal_setting(original, TerminalSettingEdit::ScrollOnOutput(true))?;
    assert!(
        SettingsResolver::resolve(Some(&enabled), None, None)?
            .scroll_on_output
            .value
    );
    assert!(
        !SettingsResolver::resolve(Some(&enabled), None, None)?
            .scroll_on_input
            .value
    );
    let disabled = set_terminal_setting(&enabled, TerminalSettingEdit::ScrollOnOutput(false))?;
    assert!(
        !SettingsResolver::resolve(Some(&disabled), None, None)?
            .scroll_on_output
            .value
    );
    let scoped = SettingsResolver::resolve(
        Some(&enabled),
        Some("[terminal]\nscroll_on_output=true"),
        Some("[terminal]\nscroll_on_output=false"),
    )?;
    assert!(!scoped.scroll_on_output.value);
    assert_eq!(scoped.scroll_on_output.source, SettingsSource::Host);
    let reset = remove_terminal_setting(&enabled, TerminalSettingKey::ScrollOnOutput)?;
    assert_eq!(reset.trim_end(), original.trim_end());
    assert!(
        !SettingsResolver::resolve(Some(&reset), None, None)?
            .scroll_on_output
            .value
    );
    for value in ["0", "\"true\"", "[]"] {
        assert!(SettingsPatch::parse(&format!("[terminal]\nscroll_on_output={value}")).is_err());
    }
    Ok(())
}

#[test]
fn mac_option_setting_round_trip_and_inheritance() -> TestResult {
    use crate::{
        settings_edit::{TerminalSettingEdit, set_terminal_setting},
        settings_remove::{TerminalSettingKey, remove_terminal_setting},
    };
    let original = "# preserve\n[terminal]\nfont_size=18\nscroll_on_input=false\n";
    assert!(
        !SettingsResolver::resolve(None, None, None)?
            .mac_option_as_meta
            .value
    );
    let enabled = set_terminal_setting(original, TerminalSettingEdit::MacOptionAsMeta(true))?;
    assert!(
        SettingsResolver::resolve(Some(&enabled), None, None)?
            .mac_option_as_meta
            .value
    );
    let disabled = set_terminal_setting(&enabled, TerminalSettingEdit::MacOptionAsMeta(false))?;
    assert!(
        !SettingsResolver::resolve(Some(&disabled), None, None)?
            .mac_option_as_meta
            .value
    );
    let scoped = SettingsResolver::resolve(
        Some(&disabled),
        Some("[terminal]\nmac_option_as_meta=true"),
        Some("[terminal]\nmac_option_as_meta=false"),
    )?;
    assert!(!scoped.mac_option_as_meta.value);
    assert_eq!(scoped.mac_option_as_meta.source, SettingsSource::Host);
    let reset = remove_terminal_setting(&enabled, TerminalSettingKey::MacOptionAsMeta)?;
    assert_eq!(reset.trim_end(), original.trim_end());
    assert!(
        !SettingsResolver::resolve(Some(&reset), None, None)?
            .scroll_on_input
            .value
    );
    for value in ["0", "\"true\"", "[]"] {
        assert!(SettingsPatch::parse(&format!("[terminal]\nmac_option_as_meta={value}")).is_err());
    }
    Ok(())
}

#[test]
fn backspace_setting_roundtrip_inheritance_and_invalid_values()
-> Result<(), Box<dyn std::error::Error>> {
    use crate::settings_edit::{TerminalSettingEdit, set_terminal_setting};
    use crate::settings_remove::{TerminalSettingKey, remove_terminal_setting};
    let original = "# retain\n[terminal]\nfont_size=14 # size\nscroll_on_output=false\n";
    assert_eq!(
        SettingsResolver::resolve(None, None, None)?
            .backspace_send
            .value,
        "del"
    );
    let saved = set_terminal_setting(original, TerminalSettingEdit::BackspaceSend("bs".into()))?;
    assert_eq!(
        SettingsResolver::resolve(Some(&saved), None, None)?
            .backspace_send
            .value,
        "bs"
    );
    assert!(saved.contains("font_size=14 # size"));
    let inherited = SettingsResolver::resolve(
        Some(&saved),
        Some("[terminal]\nbackspace_send='del'"),
        Some("[terminal]\nbackspace_send='bs'"),
    )?;
    assert_eq!(inherited.backspace_send.value, "bs");
    assert_eq!(inherited.backspace_send.source, SettingsSource::Host);
    let reset = remove_terminal_setting(&saved, TerminalSettingKey::BackspaceSend)?;
    assert_eq!(reset.trim_end(), original.trim_end());
    assert!(
        !SettingsResolver::resolve(Some(&reset), None, None)?
            .scroll_on_output
            .value
    );
    for invalid in ["\"ctrl-h\"", "\"\\u001b\"", "true", "7", "[]"] {
        assert!(SettingsPatch::parse(&format!("[terminal]\nbackspace_send={invalid}")).is_err());
    }
    assert!(
        set_terminal_setting(
            original,
            TerminalSettingEdit::BackspaceSend("arbitrary".into())
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn terminal_behavior_settings_round_trip_and_inherit() -> TestResult {
    use crate::{
        settings_edit::{TerminalSettingEdit as E, set_terminal_setting},
        settings_remove::{TerminalSettingKey as K, remove_terminal_setting},
    };
    let defaults = SettingsResolver::resolve(None, None, None)?;
    assert_eq!(defaults.delete_send.value, "csi");
    assert_eq!(defaults.backspace_send.value, "del");
    assert!(!defaults.copy_on_select.value);
    assert_eq!(defaults.term_type.value, TerminalType::Xterm256color);
    let text = set_terminal_setting(
        "# retained\n[terminal]\nfont_size=19\n",
        E::DeleteSend("del".into()),
    )?;
    let text = set_terminal_setting(&text, E::TermType(TerminalType::Vt100))?;
    let value = SettingsResolver::resolve(
        Some(&text),
        Some("[terminal]\ndelete_send='csi'"),
        Some("[terminal]\nterm_type='xterm'"),
    )?;
    assert_eq!(value.delete_send.value, "csi");
    assert_eq!(value.terminal_startup(false).term_type, TerminalType::Xterm);
    let cleared =
        remove_terminal_setting(&remove_terminal_setting(&text, K::DeleteSend)?, K::TermType)?;
    assert!(cleared.contains("# retained"));
    assert_eq!(
        SettingsResolver::resolve(Some(&cleared), None, None)?
            .font_size
            .value,
        19
    );
    assert_eq!(
        SettingsResolver::resolve(Some(&cleared), None, None)?
            .term_type
            .value,
        TerminalType::Xterm256color
    );
    for value in ["esc", "bs", "raw"] {
        assert!(SettingsPatch::parse(&format!("[terminal]\ndelete_send='{value}'")).is_err());
    }
    assert!(SettingsPatch::parse("[terminal]\nterm_type='custom-shell'").is_err());
    Ok(())
}
