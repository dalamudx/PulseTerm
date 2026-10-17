//! Isolated subprocess keymap fixture. Explicit private test directory, never user discovery.
use pulse_store::{
    keymap::{KeymapAction, KeymapOverride},
    keymap_file::{KeymapFile, KeymapFileError, KeymapMutation},
};
use std::io::{self, BufRead, Read, Write};
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let directory = args.next().ok_or("fixture directory required")?;
    let mode = args.next().ok_or("fixture mode required")?;
    if args.next().is_some() {
        return Err("unexpected fixture input".into());
    }
    let file = KeymapFile::open_private(directory.into())?;
    let expected = file.load()?;
    if mode == "reopen" {
        let projection = expected.projection()?;
        if !projection.bindings.iter().any(|binding| {
            binding.action == KeymapAction::Settings && binding.keys == "ctrl+shift+s"
        }) || projection
            .bindings
            .iter()
            .any(|binding| binding.action == KeymapAction::Hosts)
        {
            return Err("fixture durable state mismatch".into());
        }
        println!("keymap.fixture.reopened");
        return Ok(());
    }
    if !matches!(mode.as_str(), "first" | "stale") {
        return Err("invalid fixture mode".into());
    }
    println!("keymap.fixture.loaded");
    io::stdout().flush()?;
    let mut command = String::new();
    io::stdin().lock().take(16).read_line(&mut command)?;
    if command != "save\n" {
        return Err("invalid fixture signal".into());
    }
    let (action, key) = if mode == "first" {
        (KeymapAction::Settings, "ctrl+shift+s")
    } else {
        (KeymapAction::Hosts, "ctrl+shift+h")
    };
    let result = file.save_authorized(
        &expected,
        KeymapMutation::Set(KeymapOverride {
            action,
            keys: vec![key.into()],
            when: "workspaceFocus && !modalOpen".into(),
        }),
        || true,
    );
    match (mode.as_str(), result) {
        ("first", Ok(_)) => println!("keymap.fixture.committed"),
        ("stale", Err(KeymapFileError::Conflict)) => println!("keymap.fixture.stale_rejected"),
        _ => return Err("unexpected fixture outcome".into()),
    }
    Ok(())
}
