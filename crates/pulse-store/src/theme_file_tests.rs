use super::*;
use crate::private_file::Stage;
use crate::theme::{
    AccentChoice, NeutralScale, ThemeAppearance, ThemeImportFormat, UiThemeBase, import_theme,
};
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn dir() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700))?;
    Ok(d)
}
fn file(d: &Path, id: Uuid) -> PathBuf {
    d.join(format!("{id}.theme.toml"))
}
fn write(p: &Path, t: &[u8]) -> std::io::Result<()> {
    fs::write(p, t)?;
    fs::set_permissions(p, fs::Permissions::from_mode(0o600))
}
fn doc(name: &str) -> std::result::Result<ThemeDocument, Box<dyn std::error::Error>> {
    let mut p = serde_json::Map::new();
    for k in [
        "background",
        "foreground",
        "cursorColor",
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ] {
        p.insert(
            k.into(),
            serde_json::Value::String(
                if k == "background" {
                    "#000000"
                } else {
                    "#ffffff"
                }
                .into(),
            ),
        );
    }
    p.insert("name".into(), serde_json::Value::String(name.into()));
    let i = import_theme(
        ThemeImportFormat::WindowsTerminal,
        &serde_json::to_string(&p)?,
        None,
    )?;
    Ok(ThemeDocument::from_import(
        &i,
        ThemeAppearance::Dark,
        UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?),
    )?)
}
#[test]
fn create_reopen_roundtrip_display_names_are_never_paths() -> TestResult {
    let d = dir()?;
    let id = Uuid::from_u128(7);
    let f = ThemeFile::open_private(d.path().into(), id)?;
    let missing = f.load()?;
    assert!(!missing.exists());
    assert!(missing.document()?.is_none());
    let document = doc("../display-name.theme.toml")?;
    let saved = f.save(&missing, &document)?;
    assert!(saved.document()? == Some(document));
    assert_eq!(f.id(), id);
    assert!(file(d.path(), id).exists());
    assert_eq!(fs::read_dir(d.path())?.count(), 2);
    let reopened = ThemeFile::open_private(d.path().into(), id)?.load()?;
    assert!(reopened == saved);
    Ok(())
}
#[test]
fn missing_snapshots_are_bound_to_uuid_and_directory() -> TestResult {
    let d = dir()?;
    let other = dir()?;
    let a = ThemeFile::open_private(d.path().into(), Uuid::from_u128(1))?;
    let b = ThemeFile::open_private(d.path().into(), Uuid::from_u128(2))?;
    let c = ThemeFile::open_private(other.path().into(), a.id())?;
    let snap = a.load()?;
    for target in [&b, &c] {
        assert!(matches!(
            target.save(&snap, &doc("x")?),
            Err(ThemeFileError::Location)
        ));
        assert!(
            !file(
                if target.id() == b.id() {
                    d.path()
                } else {
                    other.path()
                },
                target.id()
            )
            .exists()
        );
    }
    assert!(matches!(
        ThemeFile::open_private(d.path().into(), Uuid::nil()),
        Err(ThemeFileError::Id)
    ));
    Ok(())
}
#[test]
fn no_op_preserves_identity_but_replaced_inode_never_revives() -> TestResult {
    let d = dir()?;
    let id = Uuid::from_u128(1);
    let a = ThemeFile::open_private(d.path().into(), id)?;
    let b = ThemeFile::open_private(d.path().into(), id)?;
    let initial = a.load()?;
    let one = a.save(&initial, &doc("one")?)?;
    let same = a.save(&one, &doc("one")?)?;
    assert!(one == same);
    assert!(matches!(
        b.save(&initial, &doc("two")?),
        Err(ThemeFileError::Conflict)
    ));
    let replacement = d.path().join("replacement");
    write(&replacement, one.text().as_bytes())?;
    fs::rename(replacement, file(d.path(), id))?;
    assert!(matches!(
        a.save(&one, &doc("one")?),
        Err(ThemeFileError::Conflict)
    ));
    let fresh = b.load()?;
    b.save(&fresh, &doc("two")?)?;
    assert!(matches!(
        a.save(&fresh, &doc("one")?),
        Err(ThemeFileError::Conflict)
    ));
    Ok(())
}
#[test]
fn invalid_existing_theme_utf8_budget_and_empty_are_not_reset() -> TestResult {
    let d = dir()?;
    let id = Uuid::from_u128(1);
    let f = ThemeFile::open_private(d.path().into(), id)?;
    for (data, expected) in [
        (Vec::new(), ThemeFileError::Document(ThemeError::Document)),
        (
            b"appearance='auto'".to_vec(),
            ThemeFileError::Document(ThemeError::Document),
        ),
        (vec![255], ThemeFileError::Io),
        (
            vec![b'#'; crate::theme::MAX_SCHEME_BYTES + 1],
            ThemeFileError::Document(ThemeError::Budget),
        ),
    ] {
        write(&file(d.path(), id), &data)?;
        assert!(matches!(f.load(), Err(actual) if actual == expected));
        assert_eq!(fs::read(file(d.path(), id))?, data);
    }
    Ok(())
}
#[test]
fn alias_permissions_and_readonly_directory_are_refused_without_repair() -> TestResult {
    for name in [
        "00000000-0000-0000-0000-000000000001.theme.toml",
        ".themes.lock",
    ] {
        let d = dir()?;
        let id = Uuid::from_u128(1);
        let outside = d.path().join("other");
        write(&outside, b"unchanged")?;
        symlink(&outside, d.path().join(name))?;
        let f = ThemeFile::open_private(d.path().into(), id)?;
        assert!(matches!(f.load(), Err(ThemeFileError::Location)));
        fs::remove_file(d.path().join(name))?;
        fs::hard_link(&outside, d.path().join(name))?;
        assert!(matches!(f.load(), Err(ThemeFileError::Location)));
        assert_eq!(fs::read(&outside)?, b"unchanged");
        fs::remove_file(d.path().join(name))?;
        write(&d.path().join(name), b"unchanged")?;
        fs::set_permissions(d.path().join(name), fs::Permissions::from_mode(0o400))?;
        assert!(matches!(f.load(), Err(ThemeFileError::Location)));
        assert_eq!(
            fs::metadata(d.path().join(name))?.permissions().mode() & 0o777,
            0o400
        );
    }
    let d = dir()?;
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o500))?;
    assert!(matches!(
        ThemeFile::open_private(d.path().into(), Uuid::from_u128(1)),
        Err(ThemeFileError::Location)
    ));
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700))?;
    Ok(())
}
#[test]
fn shared_themes_lock_and_failed_vs_uncertain_install_are_distinct() -> TestResult {
    let d = dir()?;
    let id = Uuid::from_u128(1);
    let a = ThemeFile::open_private(d.path().into(), id)?;
    let b = ThemeFile::open_private(d.path().into(), Uuid::from_u128(2))?;
    let first = a.load()?;
    let guard = a.inner.lock().map_err(ThemeFileError::from)?;
    assert!(matches!(b.load(), Err(ThemeFileError::Busy)));
    drop(guard);
    let text = doc("one")?.to_toml()?;
    assert!(matches!(
        a.inner.save(&first.0, text.clone(), validate, |stage| {
            if matches!(stage, Stage::TempSynced) {
                Err(ThemeFileError::Io)
            } else {
                Ok(())
            }
        }),
        Err(ThemeFileError::Io)
    ));
    assert!(!file(d.path(), id).exists());
    assert_eq!(fs::read_dir(d.path())?.count(), 1);
    assert!(matches!(
        a.inner.save(&first.0, text, validate, |stage| {
            if matches!(stage, Stage::Installed) {
                Err(ThemeFileError::Io)
            } else {
                Ok(())
            }
        }),
        Err(ThemeFileError::Uncertain)
    ));
    assert!(a.load()?.document()? == Some(doc("one")?));
    Ok(())
}
