//! Fixed in-memory keymap interoperability corpus. Never reads user configuration.
use pulse_store::keymap::{KeymapAction, KeymapOverride, KeymapPatch};
use serde_json::json;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let documents = [
        "schema_version = 1",
        "schema_version = 1\n[[bindings]]\naction = 'palette'\nkeys = []",
        "schema_version = 1\n[[bindings]]\naction = 'settings'\nkeys = [' Shift+CTRL+S ', 'meta+shift+s']\nwhen = 'workspaceFocus && !editableFocus && !modalOpen'",
        "schema_version = 1\n[[bindings]]\naction = 'hosts'\nkeys = ['ctrl+k ctrl+s']\nwhen = 'workspaceFocus && !terminalFocus'",
        "schema_version = 1\n[[bindings]]\naction = 'settings'\nkeys = ['ctrl+shift+s']\nwhen = 'terminalFocus'\n[[bindings]]\naction = 'hosts'\nkeys = ['ctrl+shift+s']\nwhen = '!terminalFocus'",
        "schema_version = 1\n[[bindings]]\naction = 'close-session'\nkeys = ['ctrl+shift+x']\nwhen = 'terminalFocus && !modalOpen'",
        "schema_version = 1\n[[bindings]]\naction = 'find'\nkeys = []",
        "schema_version = 1\n[[bindings]]\naction = 'find'\nkeys = ['ctrl+shift+g']\nwhen = 'workspaceFocus && terminalFocus && !modalOpen && !broadcast'",
    ];
    let projections = documents
        .into_iter()
        .map(|text| KeymapPatch::parse(text)?.resolve())
        .collect::<Result<Vec<_>, _>>()?;
    let mut checks = Vec::new();
    for flags in 0..16 {
        for key in b"abcdefghijklmnopqrstuvwxyz0123456789" {
            let keys = ["ctrl", "meta", "alt", "shift"]
                .into_iter()
                .enumerate()
                .filter_map(|(index, name)| (flags & (1 << index) != 0).then_some(name))
                .chain(std::iter::once(std::str::from_utf8(std::slice::from_ref(
                    key,
                ))?))
                .collect::<Vec<_>>()
                .join("+");
            for when in ["terminalFocus", "!terminalFocus"] {
                let patch = KeymapPatch {
                    schema_version: 1,
                    bindings: vec![KeymapOverride {
                        action: KeymapAction::Settings,
                        keys: vec![keys.clone()],
                        when: when.into(),
                    }],
                };
                checks.push(json!({"binding":{"action":"settings","keys":keys,"when":when},"accepted":patch.resolve().is_ok()}));
            }
        }
    }
    for keys in [
        "ctrl+shift+k ctrl+shift+s",
        "ctrl+k ctrl+s",
        "ctrl++s",
        "ctrl+ctrl+s",
        "ctrl+f13",
        "ctrl+é",
    ] {
        let patch = KeymapPatch {
            schema_version: 1,
            bindings: vec![KeymapOverride {
                action: KeymapAction::Settings,
                keys: vec![keys.into()],
                when: "terminalFocus && !broadcast".into(),
            }],
        };
        checks.push(json!({"binding":{"action":"settings","keys":keys,"when":"terminalFocus && !broadcast"},"accepted":patch.resolve().is_ok()}));
    }
    println!(
        "{}",
        serde_json::to_string(&json!({"version":1,"projections":projections,"checks":checks}))?
    );
    Ok(())
}
