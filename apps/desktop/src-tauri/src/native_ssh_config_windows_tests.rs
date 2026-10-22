//! Same-source Windows fixtures: no service, symlink privilege or connection authority.
use super::*;
use pulse_platform::windows::PrivateDirectory;
use pulse_ssh::config::ConfigQuery;
use std::fs;
type TestResult = Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    container: tempfile::TempDir,
    root: PathBuf,
}
impl Fixture {
    fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let container = tempfile::Builder::new()
            .prefix("pt-config-native.")
            .tempdir()?;
        let root = container.path().join("source");
        drop(PrivateDirectory::create(&root)?);
        drop(PrivateDirectory::create(root.join("parts"))?);
        let result = Self { container, root };
        result.put("config", b"Host other\nInclude absent/*.conf\nHost prod\nHostName example.test\nInclude parts/*.conf\nPort 2200\nIdentityFile id_ed25519\n")?;
        result.put("parts/a.conf", b"User alice\n")?;
        result.put("parts/z.conf", b"User later\n")?;
        result.put("id_ed25519", b"\xff not configuration or a real key")?;
        result.put("unreferenced", b"\xff not configuration")?;
        Ok(result)
    }
    fn put(&self, name: &str, bytes: &[u8]) -> TestResult {
        let path = self.root.join(name);
        let directory = PrivateDirectory::open(path.parent().ok_or("missing parent")?)?;
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("invalid test basename")?;
        drop(directory.create_file(name, bytes, 256 * 1024)?);
        Ok(())
    }
    fn close(self) -> TestResult {
        self.container.close()?;
        Ok(())
    }
}
fn query() -> Result<ConfigQuery<'static>, ConfigError> {
    ConfigQuery::new("prod", "local", None, None)
}
#[test]
fn bounded_discovery_preserves_context_without_reading_identity_hints() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = open_reader(&fixture.root)?;
    let snapshot: ConfigFileSnapshot = reader.capture_discovered(&query()?)?;
    let value = snapshot.resolve(&query()?)?;
    assert_eq!(value.user(), "alice");
    assert_eq!(value.target().host(), "example.test");
    assert_eq!(value.target().port(), 2200);
    assert_eq!(value.identity_files(), &["id_ed25519"]);
    reader.confirm(&snapshot)?;
    reader.confirm(&snapshot)?;
    assert!(fs::read(fixture.root.join("id_ed25519"))?.starts_with(b"\xff"));
    fixture.close()
}
#[test]
fn foreign_reader_query_and_changed_sources_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = open_reader(&fixture.root)?;
    let snapshot = reader.capture_discovered(&query()?)?;
    let other = open_reader(&fixture.root)?;
    assert_eq!(other.confirm(&snapshot), Err(ConfigError::SourceChanged));
    let foreign = ConfigQuery::new("other", "local", None, None)?;
    assert!(matches!(
        snapshot.resolve(&foreign),
        Err(ConfigError::SourceChanged)
    ));
    fs::write(fixture.root.join("parts/a.conf"), b"User carol\n")?;
    assert_eq!(reader.confirm(&snapshot), Err(ConfigError::SourceChanged));
    let current = reader.capture_discovered(&query()?)?;
    assert_eq!(current.resolve(&query()?)?.user(), "carol");
    fixture.put("parts/new.conf", b"User later\n")?;
    assert_eq!(reader.confirm(&current), Err(ConfigError::SourceChanged));
    fixture.close()
}
#[test]
fn directory_and_file_replacements_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = open_reader(&fixture.root)?;
    let snapshot = reader.capture_discovered(&query()?)?;
    let directory = ReadOnlyDirectory::open(&fixture.root.join("parts"))?;
    let (_, first) = directory.source("a.conf", 256 * 1024)?;
    fs::rename(
        fixture.root.join("parts/a.conf"),
        fixture.root.join("parts/a.old"),
    )?;
    fixture.put("parts/a.conf", b"User alice\n")?;
    let (_, second) = directory.source("a.conf", 256 * 1024)?;
    assert!(!first.same_identity(&second));
    assert_eq!(reader.confirm(&snapshot), Err(ConfigError::SourceChanged));
    drop(directory);
    fs::rename(&fixture.root, fixture.container.path().join("original"))?;
    drop(PrivateDirectory::create(&fixture.root)?);
    assert!(matches!(
        reader.capture_discovered(&query()?),
        Err(ConfigError::SourceChanged)
    ));
    fixture.close()
}
#[test]
fn windows_aliases_hardlinks_and_excess_budgets_are_rejected() -> TestResult {
    let fixture = Fixture::new()?;
    let reader = open_reader(&fixture.root)?;
    for name in [
        "../outside",
        "parts/../config",
        "C:/outside",
        "config:stream",
        "CON",
        "parts/a.conf.",
    ] {
        assert!(reader.capture(&[name]).is_err());
    }
    let directory = ReadOnlyDirectory::open(&fixture.root)?;
    assert!(matches!(
        directory.members(0, 512 * 1024),
        Err(PrivateFileError::Limit)
    ));
    assert!(matches!(
        directory.members(4096, 0),
        Err(PrivateFileError::Limit)
    ));
    assert!(matches!(
        directory.source("config", 1),
        Err(PrivateFileError::Limit)
    ));
    drop(directory);
    fs::hard_link(
        fixture.root.join("config"),
        fixture.container.path().join("linked"),
    )?;
    assert!(matches!(
        reader.capture_discovered(&query()?),
        Err(ConfigError::SourceUnavailable)
    ));
    fixture.close()
}
