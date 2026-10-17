use super::*;
fn patch(bindings: Vec<KeymapOverride>) -> KeymapPatch {
    KeymapPatch {
        schema_version: 1,
        bindings,
    }
}
fn entry(action: KeymapAction, keys: &[&str], when: &str) -> KeymapOverride {
    KeymapOverride {
        action,
        keys: keys.iter().map(|key| (*key).into()).collect(),
        when: when.into(),
    }
}
#[test]
fn missing_actions_inherit_and_empty_keys_disable_then_restore() -> Result<(), KeymapError> {
    let defaults = KeymapPatch::parse("schema_version = 1")?.resolve()?;
    assert_eq!(defaults.bindings.len(), 4);
    let mut overridden = patch(vec![entry(KeymapAction::Palette, &[], "")]);
    let disabled = overridden.resolve()?;
    assert_eq!(disabled.bindings.len(), 2);
    assert!(
        disabled
            .bindings
            .iter()
            .all(|binding| binding.action == KeymapAction::Find)
    );
    assert!(
        disabled
            .sources
            .iter()
            .any(|source| source.action == KeymapAction::Palette
                && source.source == KeymapSource::User
                && source.disabled)
    );
    overridden.restore_action(KeymapAction::Palette);
    assert_eq!(overridden.resolve()?, defaults);
    Ok(())
}
#[test]
fn toml_roundtrip_emits_only_fixed_metadata() -> Result<(), Box<dyn std::error::Error>> {
    let text = "schema_version = 1\n[[bindings]]\naction = 'settings'\nkeys = [' Shift+CTRL+S ', 'meta+shift+s']\nwhen = 'workspaceFocus && !editableFocus && !modalOpen'\n";
    let original = KeymapPatch::parse(text)?;
    assert_eq!(KeymapPatch::parse(&toml::to_string(&original)?)?, original);
    let json = serde_json::to_value(original.resolve()?)?;
    assert_eq!(json["version"], 1);
    assert_eq!(json["bindings"][0]["keys"], "ctrl+shift+s");
    assert_eq!(json["bindings"][0]["action"], "settings");
    assert_eq!(
        json["sources"].as_array().map(Vec::len),
        Some(ACTIONS.len())
    );
    assert!(json.get("run").is_none());
    Ok(())
}
#[test]
fn documents_reject_unknown_missing_version_and_oversize() {
    for text in [
        "",
        "schema_version = 1\npath = '/bin/sh'",
        "schema_version = 1\n[[bindings]]\naction = 'exec'\nkeys = []",
        "schema_version = 1\n[[bindings]]\naction = 'settings'\nkeys = []\npermission = true",
    ] {
        assert!(KeymapPatch::parse(text).is_err());
    }
    assert_eq!(
        KeymapPatch::parse("schema_version = 2"),
        Err(KeymapError::Version)
    );
    assert_eq!(
        KeymapPatch::parse(&" ".repeat(KEYMAP_TEXT_LIMIT + 1)),
        Err(KeymapError::Size)
    );
}
#[test]
fn shell_and_product_keys_remain_reserved() {
    for keys in ["ctrl+k ctrl+s", "ctrl+p", "p", "ctrl+meta+shift+s"] {
        assert_eq!(
            patch(vec![entry(
                KeymapAction::Settings,
                &[keys],
                "terminalFocus && !broadcast"
            )])
            .resolve(),
            Err(KeymapError::TerminalKey)
        );
    }
    for keys in [
        "ctrl+shift+c",
        "meta+shift+v",
        "ctrl+shift+r",
        "ctrl+shift+p",
    ] {
        assert_eq!(
            patch(vec![entry(KeymapAction::Settings, &[keys], "")]).resolve(),
            Err(KeymapError::ProductKey)
        );
    }
    assert!(
        patch(vec![entry(
            KeymapAction::Settings,
            &["ctrl+k ctrl+s"],
            "workspaceFocus && !terminalFocus"
        )])
        .resolve()
        .is_ok()
    );
    assert!(
        patch(vec![entry(
            KeymapAction::Settings,
            &["ctrl+shift+k ctrl+shift+s"],
            "terminalFocus && !broadcast"
        )])
        .resolve()
        .is_ok()
    );
}
#[test]
fn precise_and_prefix_conflicts_are_checked_after_merging() {
    for right in ["ctrl+shift+s", "ctrl+shift+s ctrl+shift+h"] {
        assert_eq!(
            patch(vec![
                entry(KeymapAction::Settings, &["ctrl+shift+s"], ""),
                entry(KeymapAction::Hosts, &[right], "")
            ])
            .resolve(),
            Err(KeymapError::Conflict)
        );
    }
    assert!(
        patch(vec![
            entry(KeymapAction::Settings, &["ctrl+shift+s"], "terminalFocus"),
            entry(KeymapAction::Hosts, &["ctrl+shift+s"], "!terminalFocus")
        ])
        .resolve()
        .is_ok()
    );
    // A settings sequence also cannot steal the still-inherited palette prefix.
    assert_eq!(
        patch(vec![entry(
            KeymapAction::Palette,
            &["ctrl+shift+p", "ctrl+shift+p ctrl+shift+s"],
            ""
        )])
        .resolve(),
        Err(KeymapError::Conflict)
    );
}
#[test]
fn invalid_disabled_condition_and_duplicate_action_fail_as_whole() {
    for when in [
        "__proto__",
        "terminalFocus || broadcast",
        "terminalFocus && !terminalFocus",
        "terminalFocus &&",
        "window.focus()",
        "! terminalFocus",
    ] {
        assert!(
            patch(vec![entry(KeymapAction::Settings, &[], when)])
                .resolve()
                .is_err()
        );
    }
    assert_eq!(
        patch(vec![
            entry(KeymapAction::Settings, &[], ""),
            entry(KeymapAction::Settings, &[], "")
        ])
        .resolve(),
        Err(KeymapError::Binding)
    );
    assert!(
        patch(vec![entry(
            KeymapAction::Settings,
            &["ctrl+shift+a"; 5],
            ""
        )])
        .resolve()
        .is_err()
    );
}
#[test]
fn syntax_matches_bounded_frontend_grammar() {
    for keys in [
        "",
        " ",
        "ctrl++p",
        "ctrl+ctrl+p",
        "ctrl+p+shift",
        "ctrl+é",
        "super+p",
        "ctrl+f13",
        "ctrl+a\nctrl+b",
        "ctrl+a ctrl+b ctrl+c",
    ] {
        assert!(sequence(keys).is_err());
    }
    for keys in ["F12", " Shift+CTRL+P  ctrl+s ", "ctrl+arrowleft", "space"] {
        assert!(sequence(keys).is_ok());
    }
    assert!(sequence(&"a".repeat(129)).is_err());
    assert!(condition(&"x".repeat(129)).is_err());
}
#[test]
fn all_modifier_and_ascii_key_combinations_roundtrip() -> Result<(), KeymapError> {
    for flags in 0..16 {
        for key in b"abcdefghijklmnopqrstuvwxyz0123456789" {
            let chord = Chord {
                modifiers: std::array::from_fn(|index| flags & (1 << index) != 0),
                key: char::from(*key).to_string(),
            };
            assert_eq!(sequence(&chord.canonical())?, vec![chord]);
        }
    }
    Ok(())
}
#[test]
fn total_merged_alias_budget_is_enforced() {
    let actions = [
        KeymapAction::Profiles,
        KeymapAction::Settings,
        KeymapAction::Hosts,
        KeymapAction::Identities,
    ];
    let entries = actions
        .into_iter()
        .zip([
            ['a', 'b', 'd', 'e'],
            ['f', 'g', 'h', 'i'],
            ['j', 'k', 'l', 'm'],
            ['n', 'o', 'q', 's'],
        ])
        .map(|(action, letters)| KeymapOverride {
            action,
            keys: letters
                .into_iter()
                .map(|key| format!("ctrl+shift+{key}"))
                .collect(),
            when: String::new(),
        })
        .collect();
    // 16 user aliases + 2 inherited aliases exceeds the effective 16 limit.
    assert_eq!(patch(entries).resolve(), Err(KeymapError::Binding));
}

#[test]
fn find_defaults_can_be_disabled_rebound_and_restored() -> Result<(), KeymapError> {
    let defaults = patch(vec![]).resolve()?;
    let find: Vec<_> = defaults
        .bindings
        .iter()
        .filter(|b| b.action == KeymapAction::Find)
        .collect();
    assert_eq!(
        find.iter().map(|b| b.keys.as_str()).collect::<Vec<_>>(),
        ["ctrl+shift+f", "meta+shift+f"]
    );
    assert!(find.iter().all(|b| b.when == FIND_WHEN));
    let mut custom = patch(vec![entry(KeymapAction::Find, &[], "")]);
    let disabled = custom.resolve()?;
    assert!(
        !disabled
            .bindings
            .iter()
            .any(|b| b.action == KeymapAction::Find)
    );
    assert!(
        disabled
            .sources
            .iter()
            .any(|s| s.action == KeymapAction::Find
                && s.source == KeymapSource::User
                && s.disabled)
    );
    custom.bindings[0].keys = vec!["ctrl+shift+g".into()];
    let rebound = custom.resolve()?;
    assert!(
        rebound
            .bindings
            .iter()
            .any(|b| b.action == KeymapAction::Find && b.keys == "ctrl+shift+g")
    );
    assert!(!rebound.bindings.iter().any(|b| b.keys == "ctrl+shift+f"));
    custom.restore_action(KeymapAction::Find);
    assert_eq!(custom.resolve()?, defaults);
    Ok(())
}
#[test]
fn inherited_find_conflicts_require_explicit_override() -> Result<(), KeymapError> {
    let mut custom = patch(vec![entry(KeymapAction::Settings, &["ctrl+shift+f"], "")]);
    assert_eq!(custom.resolve(), Err(KeymapError::Conflict));
    custom.bindings.push(entry(KeymapAction::Find, &[], ""));
    assert!(custom.resolve().is_ok());
    Ok(())
}
