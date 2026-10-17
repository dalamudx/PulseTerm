//! Real OpenSSH -G differential checks over fixed metadata-only fixtures; no network/authentication.
use super::*;
use std::{
    collections::BTreeMap,
    fs,
    process::{Command, Stdio},
};
#[test]
fn real_openssh_metadata_subset_agrees_on_order_context_and_quoting()
-> Result<(), Box<dyn std::error::Error>> {
    let root = tempfile::Builder::new()
        .prefix("pt-ssh-config-diff.")
        .tempdir_in("/data/temp")?;
    let parts = root.path().join("parts");
    fs::create_dir(&parts)?;
    fs::write(parts.join("a.conf"), "User first\nHost other\nPort 99\n")?;
    fs::write(parts.join("z.conf"), "User last\n")?;
    fs::write(parts.join(".hidden.conf"), "User hidden\n")?;
    fs::create_dir(parts.join("sub"))?;
    fs::write(parts.join("sub/z.conf"), "User nested\n")?;
    let part_a = parts.join("a.conf").to_string_lossy().into_owned();
    let part_z = parts.join("z.conf").to_string_lossy().into_owned();
    let part_hidden = parts.join(".hidden.conf").to_string_lossy().into_owned();
    let part_nested = parts.join("sub/z.conf").to_string_lossy().into_owned();
    let included = [
        ProvidedConfigSource::new(&part_z, b"User last\n"),
        ProvidedConfigSource::new(&part_a, b"User first\nHost other\nPort 99\n"),
        ProvidedConfigSource::new(&part_hidden, b"User hidden\n"),
        ProvidedConfigSource::new(&part_nested, b"User nested\n"),
    ];
    let cases = [
        ("first-value", "Host prod\nHostName %h.example\nUser alice\nPort 2200\nHost *\nUser fallback\nPort 22\n".to_owned(), "prod", None, None),
        ("host-pattern-case", "Host PROD\nUser mismatch\nHost *\nUser fallback\n".to_owned(), "prod", None, None),
        ("host-input-case", "Host PROD\nUser match\nHost *\nUser fallback\n".to_owned(), "PROD", None, None),
        ("include-context", format!("Host prod\nInclude {}/*.conf\nPort 2300\nHost *\nPort 22\n", parts.display()), "prod", None, None),
        ("match", "Host prod\nHostName REAL.example\nUser Alice\nMatch host real.* originalhost prod user Alice,!root\nPort 2222\nMatch all\nIdentitiesOnly yes\n".to_owned(), "prod", None, None),
        ("override", "Host prod\nUser alice\nPort 2200\n".to_owned(), "prod", Some("cli"), Some(2222)),
        ("quotes", "User alice\nIdentityFile 'key with spaces'\nIdentityFile key#suffix # comment\nIdentitiesOnly yes\n".to_owned(), "prod", None, None),
    ];
    for (name, text, alias, user, port) in cases {
        let path = root.path().join(name);
        fs::write(&path, &text)?;
        let mut command = Command::new("ssh");
        command.args(["-G", "-F"]).arg(path).stdin(Stdio::null());
        if let Some(user) = user {
            command.args(["-l", user]);
        }
        if let Some(port) = port {
            command.args(["-p", &port.to_string()]);
        }
        let output = command.arg(alias).output()?;
        assert!(
            output.status.success(),
            "fixed ssh -G metadata fixture failed"
        );
        let raw = String::from_utf8(output.stdout)?;
        let fields: BTreeMap<_, _> = raw
            .lines()
            .filter_map(|line| line.split_once(' '))
            .collect();
        let hints: Vec<_> = raw
            .lines()
            .filter_map(|line| line.strip_prefix("identityfile "))
            .collect();
        let bundle = ConfigBundle::parse(text.as_bytes(), &included)?;
        let resolved = bundle.resolve(&ConfigQuery::new(alias, "local", user, port)?)?;
        assert_eq!(
            Some(&resolved.target().host()),
            fields.get("hostname"),
            "hostname mismatch in fixed fixture"
        );
        let expected_port = resolved.target().port().to_string();
        assert_eq!(
            fields.get("port"),
            Some(&expected_port.as_str()),
            "port mismatch in fixed fixture"
        );
        assert_eq!(
            fields.get("user"),
            Some(&resolved.user()),
            "user mismatch in fixed fixture"
        );
        if let Some(only) = resolved.identities_only() {
            assert_eq!(
                fields.get("identitiesonly"),
                Some(&if only { "yes" } else { "no" })
            );
        }
        if name == "quotes" {
            assert_eq!(
                resolved
                    .identity_files()
                    .iter()
                    .map(String::as_str)
                    .collect::<Vec<_>>(),
                hints
            );
        }
    }
    root.close()?;
    Ok(())
}
