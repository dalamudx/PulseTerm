//! Bounded CLI discovery projection, not an executable path or a startup receipt.
#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
use serde_json::{Value, json};

#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
const KINDS: &[&str] = &[
    "bash",
    "zsh",
    "fish",
    "sh",
    "pwsh",
    "powershell",
    "cmd",
    "wsl",
];

pub(super) fn valid(value: &str) -> bool {
    pulse_store::profile_default::DefaultProfileSelection::parse(value).is_some()
}

#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
pub(super) fn project<'a>(offers: impl IntoIterator<Item = (&'a str, bool)>) -> Result<Value, ()> {
    let mut seen = Vec::new();
    let mut visible = Vec::new();
    for (kind, hidden) in offers {
        if !valid(kind) || seen.contains(&kind) || seen.len() >= (KINDS.len() + 64) {
            return Err(());
        }
        seen.push(kind);
        if !hidden {
            visible.push(kind);
        }
    }
    Ok(json!({"version":1,"profiles":visible}))
}

#[cfg(any(target_os = "linux", target_os = "macos", windows, test))]
pub(super) fn decode(value: &Value) -> Result<Vec<&str>, ()> {
    let object = value.as_object().ok_or(())?;
    if object.len() != 2 || object.get("version") != Some(&json!(1)) {
        return Err(());
    }
    let profiles = object.get("profiles").and_then(Value::as_array).ok_or(())?;
    if profiles.len() > (KINDS.len() + 64) {
        return Err(());
    }
    let mut seen = Vec::new();
    for profile in profiles {
        let kind = profile.as_str().ok_or(())?;
        if !valid(kind) || seen.contains(&kind) {
            return Err(());
        }
        seen.push(kind);
    }
    Ok(seen)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn saved_ids_roundtrip_but_paths_noncanonical_ids_and_v4_ids_never_become_profiles() {
        let id = "019a0000-0000-7000-8000-000000000001";
        let projected = project([("sh", false), (id, false)]);
        assert_eq!(
            projected.as_ref().ok().and_then(|value| decode(value).ok()),
            Some(vec!["sh", id])
        );
        assert_eq!(
            project([(id, true)]),
            Ok(json!({"version":1,"profiles":[]}))
        );
        for invalid in [
            "/bin/sh",
            "../sh",
            "019A0000-0000-7000-8000-000000000001",
            "11111111-1111-4111-8111-111111111111",
        ] {
            assert!(project([(invalid, false)]).is_err());
            assert!(decode(&json!({"version":1,"profiles":[invalid]})).is_err());
        }
    }
    #[test]
    fn discovery_hides_preferences_and_roundtrips_only_known_identities() {
        let projected = project([("bash", false), ("sh", true), ("pwsh", false)]);
        assert!(projected.is_ok());
        let Ok(value) = projected else {
            return;
        };
        assert_eq!(value, json!({"version":1,"profiles":["bash","pwsh"]}));
        assert_eq!(decode(&value), Ok(vec!["bash", "pwsh"]));
        assert_eq!(
            project([("sh", true)]),
            Ok(json!({"version":1,"profiles":[]}))
        );
        assert_eq!(decode(&json!({"version":1,"profiles":[]})), Ok(vec![]));
    }
    #[test]
    fn malformed_source_does_not_create_fallback_or_partial_projection() {
        for offers in [
            vec![("sh", false), ("sh", true)],
            vec![("/bin/sh", false)],
            vec![("\u{1b}[31m", false)],
        ] {
            assert!(project(offers).is_err());
        }
    }
    #[test]
    fn client_rejects_unbounded_unknown_or_permission_shaped_replies() {
        for value in [
            json!(null),
            json!([]),
            json!({"version":2,"profiles":["sh"]}),
            json!({"version":1.0,"profiles":["sh"]}),
            json!({"version":1,"profiles":["sh","sh"]}),
            json!({"version":1,"profiles":["sh",null]}),
            json!({"version":1,"profiles":["/bin/sh"]}),
            json!({"version":1,"profiles":["\u{1b}[31m"]}),
            json!({"version":1,"profiles":["sh"],"program":"/bin/sh"}),
            json!({"version":1,"profiles":["bash","zsh","fish","sh","pwsh","sh"]}),
            json!({"version":1}),
            json!({"version":1,"profiles":{"sh":true}}),
        ] {
            assert!(decode(&value).is_err(), "unexpected valid shape");
        }
    }
}
