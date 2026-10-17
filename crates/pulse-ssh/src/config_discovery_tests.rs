use super::*;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
};
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn fixture() -> Result<tempfile::TempDir, std::io::Error> {
    let root = tempfile::Builder::new()
        .prefix("pt-config-discovery.")
        .tempdir_in("/data/temp")?;
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
fn active_wildcards_are_lexical_and_include_context_and_repeated_reads_are_stable() -> TestResult {
    let root = fixture()?;
    put(
        root.path(),
        "parts/a.conf",
        b"Host prod\nUser alice\nHost other\nPort 1\n",
    )?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture_discovered(&query()?)?;
    assert_eq!(snapshot.resolve(&query()?)?.user(), "alice");
    assert_eq!(snapshot.resolve(&query()?)?.target().port(), 2200);
    assert_eq!(snapshot.sources.len(), 3);
    reader.confirm(&snapshot)?;
    reader.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}
#[test]
fn inactive_includes_and_identity_hints_never_open_unreferenced_fifo_or_invalid_text() -> TestResult
{
    let root = fixture()?;
    put(
        root.path(),
        "config",
        b"Host other\nInclude absent/*.conf\nHost prod\nIdentityFile id_ed25519\nUser alice\n",
    )?;
    put(root.path(), "parts/a.conf", b"\xff not a config")?;
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        root.path().join("id_ed25519"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture_discovered(&query()?)?;
    assert_eq!(snapshot.sources.len(), 1);
    assert_eq!(
        snapshot.resolve(&query()?)?.identity_files(),
        &["id_ed25519"]
    );
    reader.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}
#[test]
fn wildcard_directory_components_are_private_and_ordered() -> TestResult {
    let root = fixture()?;
    fs::create_dir(root.path().join("parts-other"))?;
    fs::set_permissions(
        root.path().join("parts-other"),
        fs::Permissions::from_mode(0o700),
    )?;
    put(root.path(), "parts-other/a.conf", b"User first\n")?;
    put(root.path(), "config", b"Include parts*/*.conf\n")?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture_discovered(&query()?)?;
    assert_eq!(snapshot.resolve(&query()?)?.user(), "first");
    reader.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}
#[test]
fn new_matching_source_and_rewrite_invalidate_old_capture_and_fresh_capture_updates() -> TestResult
{
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let old = reader.capture_discovered(&query()?)?;
    put(root.path(), "parts/00.conf", b"User newest\n")?;
    assert_eq!(reader.confirm(&old), Err(ConfigError::SourceChanged));
    let fresh = reader.capture_discovered(&query()?)?;
    assert_eq!(fresh.resolve(&query()?)?.user(), "newest");
    put(root.path(), "parts/a.conf", b"User rewritten\n")?;
    assert_eq!(reader.confirm(&fresh), Err(ConfigError::SourceChanged));
    root.close()?;
    Ok(())
}
#[test]
fn selected_links_fifo_and_unsafe_directories_reject_without_repair() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    symlink("a.conf", root.path().join("parts/linked.conf"))?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::remove_file(root.path().join("parts/linked.conf"))?;
    fs::hard_link(
        root.path().join("parts/a.conf"),
        root.path().join("parts/hard.conf"),
    )?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::remove_file(root.path().join("parts/hard.conf"))?;
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        root.path().join("parts/fifo.conf"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::SourceUnavailable)
    );
    fs::remove_file(root.path().join("parts/fifo.conf"))?;
    fs::set_permissions(root.path().join("parts"), fs::Permissions::from_mode(0o755))?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::SourceUnavailable)
    );
    assert_eq!(
        fs::metadata(root.path().join("parts"))?.mode() & 0o777,
        0o755
    );
    root.close()?;
    Ok(())
}
#[test]
fn source_limit_and_directory_member_work_limit_reject() -> TestResult {
    let root = fixture()?;
    for index in 0..31 {
        put(
            root.path(),
            &format!("parts/member-{index}.conf"),
            b"# bounded\n",
        )?;
    }
    let reader = ConfigDirectoryReader::open(root.path())?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::Limit)
    );
    for index in 0..4096 {
        fs::write(
            root.path().join("parts").join(format!("unused-{index}")),
            b"",
        )?;
    }
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::Limit)
    );
    root.close()?;
    Ok(())
}
#[test]
fn active_missing_traversal_absolute_and_cycles_reject() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    put(root.path(), "config", b"Include absent/*.conf\n")?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::MissingInclude)
    );
    for directive in [
        b"Include ../elsewhere\n".as_slice(),
        b"Include /outside\n",
        b"Include ~/outside\n",
    ] {
        put(root.path(), "config", directive)?;
        assert_eq!(
            reader.capture_discovered(&query()?).err(),
            Some(ConfigError::Unsupported)
        );
    }
    put(root.path(), "config", b"Include parts/a.conf\n")?;
    put(root.path(), "parts/a.conf", b"Include parts/a.conf\n")?;
    assert_eq!(
        reader.capture_discovered(&query()?).err(),
        Some(ConfigError::IncludeCycle)
    );
    root.close()?;
    Ok(())
}
#[test]
fn discovered_snapshot_cannot_transfer_partial_catalog_to_other_queries_or_readers() -> TestResult {
    let root = fixture()?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture_discovered(&query()?)?;
    let foreign = ConfigDirectoryReader::open(root.path())?;
    assert_eq!(foreign.confirm(&snapshot), Err(ConfigError::SourceChanged));
    for query in [
        ConfigQuery::new("other", "local", None, None)?,
        ConfigQuery::new("prod", "another", None, None)?,
        ConfigQuery::new("prod", "local", Some("override"), None)?,
        ConfigQuery::new("prod", "local", None, Some(23))?,
    ] {
        assert_eq!(
            snapshot.resolve(&query).err(),
            Some(ConfigError::SourceChanged)
        );
    }
    reader.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}

#[test]
fn discovered_file_projection_agrees_with_real_openssh_and_remains_confirmable() -> TestResult {
    use std::{
        collections::BTreeMap,
        process::{Command, Stdio},
    };
    let root = fixture()?;
    put(
        root.path(),
        "parts/a.conf",
        b"User first\nHost other\nPort 99\n",
    )?;
    put(root.path(), "parts/.hidden.conf", b"User hidden\n")?;
    put(root.path(), "parts/unrelated", b"\xff not configuration")?;
    put(
        root.path(),
        "config",
        b"Host prod\nHostName %h.example\nInclude parts/*.conf\nPort 2300\n",
    )?;
    // OpenSSH resolves relative user Includes against ~/.ssh; rewrite only the inert oracle fixture.
    let oracle = format!(
        "Host prod\nHostName %h.example\nInclude {}/*.conf\nPort 2300\n",
        root.path().join("parts").display()
    );
    put(root.path(), "oracle", oracle.as_bytes())?;
    let reader = ConfigDirectoryReader::open(root.path())?;
    let snapshot = reader.capture_discovered(&query()?)?;
    let resolved = snapshot.resolve(&query()?)?;
    let output = Command::new("ssh")
        .args(["-G", "-F"])
        .arg(root.path().join("oracle"))
        .arg("prod")
        .stdin(Stdio::null())
        .output()?;
    assert!(
        output.status.success(),
        "fixed metadata-only OpenSSH fixture failed"
    );
    let raw = String::from_utf8(output.stdout)?;
    let fields: BTreeMap<_, _> = raw
        .lines()
        .filter_map(|line| line.split_once(' '))
        .collect();
    assert_eq!(fields.get("hostname"), Some(&resolved.target().host()));
    let port = resolved.target().port().to_string();
    assert_eq!(fields.get("port"), Some(&port.as_str()));
    assert_eq!(fields.get("user"), Some(&resolved.user()));
    assert_eq!(snapshot.sources.len(), 3);
    reader.confirm(&snapshot)?;
    root.close()?;
    Ok(())
}
