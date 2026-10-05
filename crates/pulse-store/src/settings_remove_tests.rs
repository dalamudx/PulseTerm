use super::*;
use crate::settings::{SettingsResolver, SettingsSource};
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn ordinary_assignment_preserves_rationale_suffix_whitespace_and_other_values() -> TestResult {
    let input = "# header\r\nschema_version=1\r\n[terminal] # table\r\n# rationale\r\n\tfont_size = 18 # size\r\nfont_family='Fira Code'\r\ncursor_blink=false\r\n";
    let actual = remove_terminal_setting(input, TerminalSettingKey::FontSize)?;
    assert_eq!(actual, input.replace("font_size = 18", ""));
    assert_eq!(
        remove_terminal_setting(&actual, TerminalSettingKey::FontSize)?,
        actual
    );
    Ok(())
}
#[test]
fn dotted_and_escaped_keys_remove_only_the_whole_assignment() -> TestResult {
    for (text, token) in [
        (
            "terminal . font_size = 18 # target\nterminal.font_family='Fira Code' # other",
            "terminal . font_size = 18",
        ),
        (
            "[terminal]\n\"font\\u005fsize\"=18 # target\nfont_family='Fira Code'",
            "\"font\\u005fsize\"=18",
        ),
    ] {
        assert_eq!(
            remove_terminal_setting(text, TerminalSettingKey::FontSize)?,
            text.replace(token, "")
        );
    }
    Ok(())
}
#[test]
fn inline_first_middle_last_and_only_remove_one_comma() -> TestResult {
    for (input, expected) in [
        (
            "terminal={font_size=18, cursor_blink=false, font_family='Fira Code'} # tail",
            "terminal={ cursor_blink=false, font_family='Fira Code'} # tail",
        ),
        (
            "terminal={cursor_blink=false, font_size=18, font_family='Fira Code'}",
            "terminal={cursor_blink=false,  font_family='Fira Code'}",
        ),
        (
            "terminal={cursor_blink=false, font_size=18} # tail",
            "terminal={cursor_blink=false } # tail",
        ),
        ("terminal={font_size=18} # tail", "terminal={} # tail"),
    ] {
        assert_eq!(
            remove_terminal_setting(input, TerminalSettingKey::FontSize)?,
            expected
        );
    }
    Ok(())
}
#[test]
fn inline_comment_commas_and_trailing_comma_are_not_delimiters() -> TestResult {
    let text = "terminal={\n font_size=18, # rationale, comma stays\n cursor_blink=false, # another, comment\n} # end\n";
    assert_eq!(
        remove_terminal_setting(text, TerminalSettingKey::FontSize)?,
        text.replace("font_size=18,", "")
    );
    assert_eq!(
        remove_terminal_setting(text, TerminalSettingKey::CursorBlink)?,
        text.replace("cursor_blink=false,", "")
    );
    Ok(())
}
#[test]
fn multiline_unicode_value_and_no_final_newline_are_preserved() -> TestResult {
    let text = "[terminal]\nfont_family=\"\"\"更\\\n  纱黑体\"\"\" # keep\nfont_size=18";
    assert_eq!(
        remove_terminal_setting(text, TerminalSettingKey::FontFamily)?,
        "[terminal]\n # keep\nfont_size=18"
    );
    Ok(())
}
#[test]
fn no_override_is_byte_identical_and_empty_table_stays() -> TestResult {
    for input in [
        "",
        "# header\n",
        "[terminal] # table\nfont_size=18\n",
        "terminal={} # keep\n",
    ] {
        assert_eq!(
            remove_terminal_setting(input, TerminalSettingKey::CursorBlink)?,
            input
        );
    }
    assert_eq!(
        remove_terminal_setting(
            "[terminal] # table\nfont_size=18",
            TerminalSettingKey::FontSize
        )?,
        "[terminal] # table\n"
    );
    Ok(())
}
#[test]
fn removal_restores_individual_lower_layer_provenance_without_defaults() -> TestResult {
    let user = "[terminal]\nfont_size=16\ncursor_blink=true\n";
    let group = "terminal={font_size=18,cursor_blink=false}";
    let host = "terminal={font_size=20,cursor_blink=false}";
    let host = remove_terminal_setting(host, TerminalSettingKey::FontSize)?;
    let resolved = SettingsResolver::resolve(Some(user), Some(group), Some(&host))?;
    assert_eq!(
        (resolved.font_size.value, resolved.font_size.source),
        (18, SettingsSource::Group)
    );
    assert_eq!(
        (resolved.cursor_blink.value, resolved.cursor_blink.source),
        (false, SettingsSource::Host)
    );
    let group = remove_terminal_setting(group, TerminalSettingKey::FontSize)?;
    assert_eq!(
        SettingsResolver::resolve(Some(user), Some(&group), Some(&host))?
            .font_size
            .source,
        SettingsSource::User
    );
    let user = remove_terminal_setting(user, TerminalSettingKey::FontSize)?;
    assert_eq!(
        SettingsResolver::resolve(Some(&user), Some(&group), Some(&host))?
            .font_size
            .source,
        SettingsSource::Default
    );
    Ok(())
}
#[test]
fn generated_layout_matrix_preserves_exact_non_target_bytes() -> TestResult {
    let permutations = [
        [0, 1, 2],
        [0, 2, 1],
        [1, 0, 2],
        [1, 2, 0],
        [2, 0, 1],
        [2, 1, 0],
    ];
    let entries = [
        ("font_size", "18", TerminalSettingKey::FontSize),
        (
            "font_family",
            "'更纱黑体 Mono'",
            TerminalSettingKey::FontFamily,
        ),
        ("cursor_blink", "false", TerminalSettingKey::CursorBlink),
    ];
    let mut checked = 0;
    for order in permutations {
        for layout in 0..3 {
            for quoted in [false, true] {
                for newline in ["\n", "\r\n"] {
                    for final_newline in [false, true] {
                        for trailing in [false, true] {
                            let mut text = format!(
                                "# literal font_size=18, 更纱; preserve{newline}schema_version = 1 # version{newline}"
                            );
                            if layout == 0 {
                                text.push_str(&format!("[terminal] # table{newline}"));
                            }
                            if layout == 2 {
                                text.push_str(&format!("terminal = {{{newline}"));
                            }
                            let mut assignments = Vec::new();
                            let mut commas = Vec::new();
                            for (position, index) in order.iter().copied().enumerate() {
                                text.push('\t');
                                let (name, value, _) = entries[index];
                                let key = if quoted {
                                    format!("\"{}\"", name.replace('_', "\\u005f"))
                                } else {
                                    name.to_owned()
                                };
                                let assignment = if layout == 1 {
                                    format!("terminal . {key} = {value}")
                                } else {
                                    format!("{key} = {value}")
                                };
                                let start = text.len();
                                text.push_str(&assignment);
                                assignments.push(start..text.len());
                                if layout == 2 && (position < 2 || trailing) {
                                    commas.push(Some(text.len()));
                                    text.push(',');
                                } else {
                                    commas.push(None);
                                }
                                text.push_str(&format!(
                                    " # literal cursor_blink=false, untouched {position}{newline}"
                                ));
                            }
                            if layout == 2 {
                                text.push_str("} # end");
                            } else {
                                text.push_str("# end");
                            }
                            if final_newline {
                                text.push_str(newline);
                            }
                            for position in 0..3 {
                                let mut ranges = vec![assignments[position].clone()];
                                if layout == 2 {
                                    let comma = commas[position]
                                        .or_else(|| {
                                            position
                                                .checked_sub(1)
                                                .and_then(|previous| commas[previous])
                                        })
                                        .ok_or("missing generated delimiter")?;
                                    ranges.push(comma..comma + 1);
                                }
                                ranges.sort_by_key(|range| std::cmp::Reverse(range.start));
                                let mut expected = text.clone();
                                for range in ranges {
                                    expected.replace_range(range, "");
                                }
                                let key = entries[order[position]].2;
                                assert_eq!(remove_terminal_setting(&text, key)?, expected);
                                assert_eq!(remove_terminal_setting(&expected, key)?, expected);
                                checked += 1;
                            }
                        }
                    }
                }
            }
        }
    }
    assert_eq!(checked, 864);
    Ok(())
}
#[test]
fn invalid_or_over_budget_document_is_never_repaired_or_reset() {
    for text in [
        "schema_version=2",
        "secret='not returned'",
        "[terminal]\nfont_size=1",
        "terminal={font_size=18,font_size=19}",
        "terminal=42",
    ] {
        assert!(remove_terminal_setting(text, TerminalSettingKey::FontSize).is_err());
    }
    assert_eq!(
        remove_terminal_setting(
            &"#".repeat(crate::settings::SETTINGS_TEXT_LIMIT + 1),
            TerminalSettingKey::FontSize
        ),
        Err(SettingsError::Size)
    );
}
