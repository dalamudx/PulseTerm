use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn fixture() -> Result<tempfile::TempDir, std::io::Error> {
    let root = tempfile::Builder::new()
        .prefix("pt-config-source.")
        .tempdir_in(std::fs::canonicalize(std::env::temp_dir())?)?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    fs::create_dir(root.path().join("parts"))?;
    fs::set_permissions(root.path().join("parts"), fs::Permissions::from_mode(0o700))?;
    put(
        root.path(),
        "config",
        b"Host prod\nInclude parts/*.conf\nPort 2200\n",
    )?;
    put(root.path(), "parts/a.conf", b"User alice\n")?;
    put(root.path(), "parts/z.conf", b"User later\n")?;
    Ok(root)
}
fn put(root: &Path, name: &str, bytes: &[u8]) -> Result<(), std::io::Error> {
    fs::write(root.join(name), bytes)?;
    fs::set_permissions(root.join(name), fs::Permissions::from_mode(0o600))
}
fn query() -> Result<ConfigQuery<'static>, ConfigError> {
    ConfigQuery::new("prod", "local", None, None)
}
#[test]
fn approved_nested_sources_resolve_in_lexical_order_and_confirm_read_only() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let before = stamp(&fs::metadata(root.path().join("parts/a.conf"))?);
    let snapshot = reader.capture(&["parts/z.conf", "parts/a.conf"])?;
    assert_eq!(snapshot.resolve(&query()?)?.user(), "alice");
    assert_eq!(snapshot.resolve(&query()?)?.target().port(), 2200);
    reader.confirm(&snapshot)?;
    assert_eq!(
        stamp(&fs::metadata(root.path().join("parts/a.conf"))?),
        before
    );
    assert_eq!(fs::read_dir(root.path().join("parts"))?.count(), 2);
    root.close()?;
    Ok(())
}
#[test]
fn same_directory_foreign_reader_cannot_confirm_original_snapshot() -> TestResult {
    let root = fixture()?;
    let first = ConfigDirectoryReader::open(root.path())?;
    let snapshot = first.capture(&["parts/a.conf"])?;
    let other = ConfigDirectoryReader::open(root.path())?;
    assert_eq!(other.confirm(&snapshot), Err(ConfigError::SourceChanged));
    first.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}
#[test]
fn rewritten_included_or_primary_source_invalidates_previous_but_new_capture_works() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let original = reader.capture(&["parts/a.conf"])?;
    put(root.path(), "parts/a.conf", b"User carol\n")?;
    assert_eq!(reader.confirm(&original), Err(ConfigError::SourceChanged));
    assert_eq!(original.resolve(&query()?)?.user(), "alice");
    let fresh = reader.capture(&["parts/a.conf"])?;
    assert_eq!(fresh.resolve(&query()?)?.user(), "carol");
    put(
        root.path(),
        "config",
        b"Host prod\nInclude parts/*.conf\nPort 2300\n",
    )?;
    assert_eq!(reader.confirm(&fresh), Err(ConfigError::SourceChanged));
    assert_eq!(
        reader
            .capture(&["parts/a.conf"])?
            .resolve(&query()?)?
            .target()
            .port(),
        2300
    );
    root.close()?;
    Ok(())
}
#[test]
fn same_bytes_inode_replacement_and_deletion_cannot_confirm() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture(&["parts/a.conf"])?;
    put(root.path(), "parts/replacement", b"User alice\n")?;
    fs::rename(
        root.path().join("parts/replacement"),
        root.path().join("parts/a.conf"),
    )?;
    assert_eq!(reader.confirm(&snapshot), Err(ConfigError::SourceChanged));
    let fresh = reader.capture(&["parts/a.conf"])?;
    fs::remove_file(root.path().join("parts/a.conf"))?;
    assert!(reader.confirm(&fresh).is_err());
    assert_eq!(
        reader.capture(&["parts/a.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    root.close()?;
    Ok(())
}
#[test]
fn new_include_member_and_directory_replacement_revoke_old_snapshot() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture(&["parts/a.conf"])?;
    put(root.path(), "parts/new.conf", b"User new\n")?;
    assert_eq!(reader.confirm(&snapshot), Err(ConfigError::SourceChanged));
    let fresh = reader.capture(&["parts/a.conf"])?;
    fs::rename(root.path().join("parts"), root.path().join("old-parts"))?;
    fs::create_dir(root.path().join("parts"))?;
    fs::set_permissions(root.path().join("parts"), fs::Permissions::from_mode(0o700))?;
    put(root.path(), "parts/a.conf", b"User alice\n")?;
    assert_eq!(reader.confirm(&fresh), Err(ConfigError::SourceChanged));
    reader.confirm(&reader.capture(&["parts/a.conf"])?)?;
    root.close()?;
    Ok(())
}
#[test]
fn unsafe_include_symlink_hardlink_fifo_and_permissions_fail_closed() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    symlink("a.conf", root.path().join("parts/link.conf"))?;
    assert_eq!(
        reader.capture(&["parts/link.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::hard_link(
        root.path().join("parts/z.conf"),
        root.path().join("parts/hard.conf"),
    )?;
    assert_eq!(
        reader.capture(&["parts/hard.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    crate::config::mkfifo(&root.path().join("parts/fifo.conf"))?;
    assert_eq!(
        reader.capture(&["parts/fifo.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::set_permissions(
        root.path().join("parts/a.conf"),
        fs::Permissions::from_mode(0o666),
    )?;
    assert_eq!(
        reader.capture(&["parts/a.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::set_permissions(
        root.path().join("parts/a.conf"),
        fs::Permissions::from_mode(0o600),
    )?;
    fs::set_permissions(root.path().join("parts"), fs::Permissions::from_mode(0o755))?;
    assert_eq!(
        reader.capture(&["parts/a.conf"]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::set_permissions(root.path().join("parts"), fs::Permissions::from_mode(0o700))?;
    root.close()?;
    Ok(())
}
#[test]
fn catalog_paths_are_prevalidated_and_budgeted_without_escape_or_expansion() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    for name in [
        "../other",
        "/etc/passwd",
        "parts/../other",
        "./parts/a.conf",
        "parts//a.conf",
        "parts/*.conf",
        "parts\\a.conf",
        "",
    ] {
        assert_eq!(reader.capture(&[name]).err(), Some(ConfigError::Syntax));
    }
    assert_eq!(
        reader.capture(&["parts/a.conf", "parts/a.conf"]).err(),
        Some(ConfigError::Syntax)
    );
    assert_eq!(reader.capture(&["config"]).err(), Some(ConfigError::Syntax));
    assert_eq!(
        reader.capture(&["a/b/c/d/e/f/g/h/i"]).err(),
        Some(ConfigError::Limit)
    );
    assert_eq!(
        reader.capture(&["parts/a.conf"; MAX_SOURCES + 1]).err(),
        Some(ConfigError::Limit)
    );
    put(root.path(), "parts/a.conf", &vec![b'#'; MAX_SOURCE])?;
    put(root.path(), "parts/z.conf", &vec![b'#'; MAX_SOURCE])?;
    assert_eq!(
        reader.capture(&["parts/a.conf", "parts/z.conf"]).err(),
        Some(ConfigError::Limit)
    );
    root.close()?;
    Ok(())
}
#[test]
fn root_directory_replacement_requires_new_reader_not_implicit_rebinding() -> TestResult {
    let parent = tempfile::Builder::new()
        .prefix("pt-config-source-root.")
        .tempdir_in(std::fs::canonicalize(std::env::temp_dir())?)?;
    let path = parent.path().join("ssh");
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    put(&path, "config", b"User alice\n")?;
    let reader = ConfigDirectoryReader::open(&path)?;
    let snapshot = reader.capture(&[])?;
    fs::rename(&path, parent.path().join("old"))?;
    fs::create_dir(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
    put(&path, "config", b"User alice\n")?;
    assert_eq!(reader.capture(&[]).err(), Some(ConfigError::SourceChanged));
    assert_eq!(reader.confirm(&snapshot), Err(ConfigError::SourceChanged));
    let new_reader = ConfigDirectoryReader::open(&path)?;
    new_reader.confirm(&new_reader.capture(&[])?)?;
    parent.close()?;
    Ok(())
}

#[test]
fn directory_enumeration_budget_rejects_excess_even_when_files_are_not_admitted() -> TestResult {
    let root = fixture()?;
    for index in 0..4096 {
        fs::write(
            root.path().join("parts").join(format!("member-{index}")),
            b"",
        )?;
    }
    let reader = ConfigDirectoryReader::open(root.path())?;
    assert_eq!(
        reader.capture(&["parts/a.conf"]).err(),
        Some(ConfigError::Limit)
    );
    root.close()?;
    Ok(())
}
