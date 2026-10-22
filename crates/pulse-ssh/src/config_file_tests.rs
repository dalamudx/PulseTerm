use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    time::Instant,
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn fixture() -> Result<tempfile::TempDir, std::io::Error> {
    let root = tempfile::Builder::new()
        .prefix("pt-config-file.")
        .tempdir_in(std::fs::canonicalize(std::env::temp_dir())?)?;
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    Ok(root)
}
fn write(path: &Path, bytes: &[u8]) -> Result<(), std::io::Error> {
    fs::write(path.join("config"), bytes)?;
    fs::set_permissions(path.join("config"), fs::Permissions::from_mode(0o600))
}
#[test]
fn fixed_read_is_read_only_and_missing_is_not_empty_config() -> TestResult {
    let root = fixture()?;
    assert_eq!(
        load(root.path(), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    assert!(!root.path().join("config").exists());
    write(
        root.path(),
        b"Host prod\nHostName real.example\nUser alice\n",
    )?;
    let before = stamp(&fs::metadata(root.path().join("config"))?);
    let resolved = load(root.path(), &[])?.resolve(&super::super::ConfigQuery::new(
        "prod", "local", None, None,
    )?)?;
    assert_eq!(resolved.target().host(), "real.example");
    assert_eq!(resolved.user(), "alice");
    assert_eq!(before, stamp(&fs::metadata(root.path().join("config"))?));
    assert_eq!(fs::read_dir(root.path())?.count(), 1);
    write(root.path(), b"")?;
    assert!(load(root.path(), &[]).is_ok());
    root.close()?;
    Ok(())
}
#[test]
fn supplied_include_is_metadata_only_and_never_auto_read_from_disk() -> TestResult {
    let root = fixture()?;
    write(root.path(), b"Include other\n")?;
    fs::write(root.path().join("other"), b"User unintended\n")?;
    let query = super::super::ConfigQuery::new("prod", "local", None, None)?;
    assert_eq!(
        load(root.path(), &[])?.resolve(&query).err(),
        Some(ConfigError::MissingInclude)
    );
    let sources = [ProvidedConfigSource::new("other", b"User supplied\n")];
    assert_eq!(
        load(root.path(), &sources)?.resolve(&query)?.user(),
        "supplied"
    );
    root.close()?;
    Ok(())
}
#[test]
fn leaf_and_parent_symlinks_and_hardlinks_are_rejected() -> TestResult {
    let root = fixture()?;
    let actual = root.path().join("actual");
    fs::create_dir(&actual)?;
    fs::set_permissions(&actual, fs::Permissions::from_mode(0o700))?;
    write(&actual, b"User alice\n")?;
    let linked = root.path().join("linked");
    symlink(&actual, &linked)?;
    assert_eq!(
        load(&linked, &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    let nested = actual.join("nested");
    fs::create_dir(&nested)?;
    fs::set_permissions(&nested, fs::Permissions::from_mode(0o700))?;
    write(&nested, b"User alice\n")?;
    assert_eq!(
        load(&linked.join("nested"), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    symlink(actual.join("config"), root.path().join("config"))?;
    assert_eq!(
        load(root.path(), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::remove_file(root.path().join("config"))?;
    fs::hard_link(actual.join("config"), root.path().join("config"))?;
    assert_eq!(
        load(root.path(), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    root.close()?;
    Ok(())
}
#[test]
fn permissions_are_rejected_without_chmod_even_under_root() -> TestResult {
    let root = fixture()?;
    write(root.path(), b"User alice\n")?;
    for mode in [0o666, 0o620, 0o000, 0o200] {
        fs::set_permissions(root.path().join("config"), fs::Permissions::from_mode(mode))?;
        assert_eq!(
            load(root.path(), &[]).err(),
            Some(ConfigError::SourceUnavailable)
        );
        assert_eq!(
            fs::metadata(root.path().join("config"))?.mode() & 0o777,
            mode
        );
    }
    fs::set_permissions(
        root.path().join("config"),
        fs::Permissions::from_mode(0o644),
    )?;
    assert!(load(root.path(), &[]).is_ok());
    for mode in [0o755, 0o777, 0o000, 0o600] {
        fs::set_permissions(root.path(), fs::Permissions::from_mode(mode))?;
        assert_eq!(
            load(root.path(), &[]).err(),
            Some(ConfigError::SourceUnavailable)
        );
        assert_eq!(fs::metadata(root.path())?.mode() & 0o777, mode);
    }
    fs::set_permissions(root.path(), fs::Permissions::from_mode(0o700))?;
    root.close()?;
    Ok(())
}
#[test]
fn fifo_directory_oversize_and_invalid_text_fail_without_blocking() -> TestResult {
    let root = fixture()?;
    fs::create_dir(root.path().join("config"))?;
    assert_eq!(
        load(root.path(), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::remove_dir(root.path().join("config"))?;
    #[cfg(not(any(target_vendor = "apple", target_os = "redox", target_os = "wasi")))]
    {
        sys::mkfifoat(
            sys::CWD,
            root.path().join("config"),
            Mode::RUSR | Mode::WUSR,
        )?;
        let start = Instant::now();
        assert_eq!(
            load(root.path(), &[]).err(),
            Some(ConfigError::SourceUnavailable)
        );
        assert!(start.elapsed() < std::time::Duration::from_secs(1));
        fs::remove_file(root.path().join("config"))?;
    }
    write(root.path(), &vec![b'#'; MAX_SOURCE + 1])?;
    assert_eq!(
        load(root.path(), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    write(root.path(), &[0xff])?;
    assert_eq!(load(root.path(), &[]).err(), Some(ConfigError::Syntax));
    assert_eq!(
        load(Path::new("relative"), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    assert_eq!(
        load(&root.path().join("../other"), &[]).err(),
        Some(ConfigError::SourceUnavailable)
    );
    root.close()?;
    Ok(())
}
