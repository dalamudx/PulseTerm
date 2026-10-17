use super::super::super::super::config_imports::ConfigImportBank;
use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt};
use uuid::Uuid;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    directory: tempfile::TempDir,
    reader: Arc<ProfileReader>,
    registry: SessionRegistry,
    documents: ProductDocuments,
    window: pulse_app::registry::WindowToken,
    page: Arc<PageLaunchScope>,
}
fn write(path: &std::path::Path, body: &[u8]) -> TestResult {
    fs::write(path, body)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
fn fixture() -> std::result::Result<Fixture, Box<dyn std::error::Error>> {
    let directory = tempfile::Builder::new()
        .prefix("pt-config-import.")
        .tempdir_in("/data/temp")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    fs::create_dir(directory.path().join("database"))?;
    fs::set_permissions(
        directory.path().join("database"),
        fs::Permissions::from_mode(0o700),
    )?;
    write(&directory.path().join("config"),b"Host prod\n HostName example.test\n Port 2222\n User alice\n IdentityFile missing-private-key\n")?;
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    Ok(Fixture {
        directory,
        reader: Arc::new(ProfileReader::new()),
        registry,
        documents,
        window,
        page,
    })
}
impl Fixture {
    fn database(&self) -> PathBuf {
        self.directory.path().join("database")
    }
    async fn prepared(&self) -> Result<Arc<ConfigImportCapture>> {
        let source = self
            .reader
            .capture_config(
                self.registry.clone(),
                self.page.clone(),
                ConfigReadRequest {
                    directory: self.directory.path().to_owned(),
                    includes: Vec::new(),
                    alias: "prod".into(),
                    local_user: "local".into(),
                },
            )
            .await?;
        self.reader
            .prepare_config_import(source, self.database(), "Imported prod".into())
            .await
    }
    async fn hosts(&self) -> Result<HostSnapshot> {
        self.reader
            .hosts(self.registry.clone(), self.page.clone(), self.database())
            .await
    }
    async fn cleanup(self) -> TestResult {
        self.reader.shutdown().await?;
        self.registry.shutdown()?;
        self.directory.close()?;
        Ok(())
    }
}
fn claimed(
    f: &Fixture,
    capture: Arc<ConfigImportCapture>,
) -> Result<(
    ConfigImportBank,
    Uuid,
    Arc<ConfigImportCapture>,
    Arc<ImportControl>,
)> {
    let bank = ConfigImportBank::new();
    let token = bank.publish(&f.registry, capture)?;
    let (capture, control) = bank.claim(&f.registry, &f.page, token)?;
    Ok((bank, token, capture, control))
}
#[tokio::test]
async fn config_import_creates_exact_endpoint_without_reading_key_and_reopens() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    assert!(f.hosts().await?.records().is_empty());
    assert_eq!(prepared.identity_file_count(), 1);
    let (bank, token, capture, control) = claimed(&f, prepared)?;
    let saved = f.reader.import_config(capture, control).await?;
    assert_eq!(saved.records().len(), 1);
    let record = &saved.records()[0];
    assert_eq!(record.endpoint().label(), "Imported prod");
    assert_eq!(record.endpoint().target().host(), "example.test");
    assert_eq!(record.endpoint().target().port(), 2222);
    assert_eq!(record.endpoint().username(), "alice");
    assert!(bank.claim(&f.registry, &f.page, token).is_err());
    f.reader.shutdown().await?;
    let other = Arc::new(ProfileReader::new());
    let reopened = other
        .hosts(f.registry.clone(), f.page.clone(), f.database())
        .await?;
    assert_eq!(reopened.records()[0].id(), record.id());
    assert!(reopened.records()[0].endpoint() == record.endpoint());
    other.shutdown().await?;
    f.cleanup().await
}
#[tokio::test]
async fn config_import_original_source_change_rejected_and_fresh_prepare_succeeds() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    write(
        &f.directory.path().join("config"),
        b"Host prod\n HostName other.test\n User alice\n",
    )?;
    let (_, _, capture, control) = claimed(&f, prepared)?;
    assert!(matches!(
        f.reader.import_config(capture, control).await,
        Err("workspace.config_conflict")
    ));
    assert!(f.hosts().await?.records().is_empty());
    let (_, _, capture, control) = claimed(&f, f.prepared().await?)?;
    let saved = f.reader.import_config(capture, control).await?;
    assert_eq!(saved.records()[0].endpoint().target().host(), "other.test");
    f.cleanup().await
}
#[tokio::test]
async fn config_import_sql_precommit_source_change_rolls_back_written_row() -> TestResult {
    let f = fixture()?;
    let (_, _, capture, control) = claimed(&f, f.prepared().await?)?;
    let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let observed = calls.clone();
    let source = f.directory.path().join("config");
    let hook = Arc::new(move || {
        if observed.fetch_add(1, Ordering::SeqCst) == 2 {
            let _changed = fs::write(&source, b"Host prod\n HostName changed.test\n");
        }
    });
    assert!(matches!(
        f.reader.import_config_inner(capture, control, hook).await,
        Err("workspace.config_conflict")
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 3);
    let snapshot = f.hosts().await?;
    assert_eq!(snapshot.revision(), 0);
    assert!(snapshot.records().is_empty());
    f.cleanup().await
}
#[tokio::test]
async fn config_import_external_database_edit_conflicts_without_overwrite() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    let current = f.hosts().await?;
    let endpoint = SshEndpoint::new("External", "outside.test", 22, "bob")?;
    f.reader
        .edit_host(
            f.registry.clone(),
            f.page.clone(),
            f.database(),
            current,
            HostEdit::Create(endpoint),
        )
        .await?;
    let (_, _, capture, control) = claimed(&f, prepared)?;
    assert!(matches!(
        f.reader.import_config(capture, control).await,
        Err("workspace.hosts_conflict")
    ));
    let saved = f.hosts().await?;
    assert_eq!(saved.records().len(), 1);
    assert_eq!(saved.records()[0].endpoint().label(), "External");
    f.cleanup().await
}
#[tokio::test]
async fn config_import_wrong_reader_wrong_ticket_cancel_and_expiry_are_denied() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    let bank = ConfigImportBank::new();
    let token = bank.publish(&f.registry, prepared.clone())?;
    assert!(bank.claim(&f.registry, &f.page, Uuid::new_v4()).is_err());
    bank.cancel(&f.registry, &f.page, token)?;
    assert!(bank.claim(&f.registry, &f.page, token).is_err());
    let expired = ImportControl {
        stage: AtomicU8::new(0),
        expires: Instant::now() - Duration::from_secs(1),
    };
    assert!(expired.claim().is_err());
    let other = Arc::new(ProfileReader::new());
    let control = ImportControl::new();
    control.claim()?;
    assert!(matches!(
        other.import_config(prepared, control).await,
        Err("workspace.denied")
    ));
    other.shutdown().await?;
    assert!(f.hosts().await?.records().is_empty());
    f.cleanup().await
}
#[tokio::test]
async fn config_import_navigation_cannot_transfer_old_intent() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    let bank = ConfigImportBank::new();
    let token = bank.publish(&f.registry, prepared.clone())?;
    f.documents.started(f.window)?;
    let nonce = f.documents.finished(f.window)?;
    f.documents.confirm(f.window, nonce)?;
    let page = f.documents.current(f.window, nonce)?;
    assert!(bank.claim(&f.registry, &page, token).is_err());
    let control = ImportControl::new();
    control.claim()?;
    assert!(matches!(
        f.reader.import_config(prepared, control).await,
        Err("workspace.denied")
    ));
    let saved = f
        .reader
        .hosts(f.registry.clone(), page, f.database())
        .await?;
    assert!(saved.records().is_empty());
    f.cleanup().await
}
#[tokio::test]
async fn config_import_unpolled_future_releases_unused_approval_without_sql() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    let (bank, _, capture, control) = claimed(&f, prepared.clone())?;
    let future = f.reader.import_config(capture, control.clone());
    drop(future);
    assert!(!control.running());
    bank.publish(&f.registry, prepared)?;
    assert!(f.hosts().await?.records().is_empty());
    f.cleanup().await
}
struct Release(Arc<(Mutex<bool>, std::sync::Condvar)>);
impl Drop for Release {
    fn drop(&mut self) {
        if let Ok(mut done) = self.0.0.lock() {
            *done = true;
            self.0.1.notify_all();
        }
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn config_import_cancelled_waiter_keeps_bank_and_io_until_native_exit() -> TestResult {
    let f = fixture()?;
    let prepared = f.prepared().await?;
    let (bank, token, capture, control) = claimed(&f, prepared.clone())?;
    let gate = Arc::new((Mutex::new(false), std::sync::Condvar::new()));
    let release = Release(gate.clone());
    let (tx, rx) = tokio::sync::oneshot::channel();
    let signal = Mutex::new(Some(tx));
    let hook = Arc::new(move || {
        if let Ok(mut value) = signal.lock()
            && let Some(sender) = value.take()
        {
            let _ = sender.send(());
        }
        if let Ok(mut done) = gate.0.lock() {
            while !*done {
                match gate.1.wait(done) {
                    Ok(value) => done = value,
                    Err(_) => return,
                }
            }
        }
    });
    let task = tokio::spawn(f.reader.import_config_inner(capture, control.clone(), hook));
    tokio::time::timeout(Duration::from_secs(5), rx).await??;
    task.abort();
    assert!(task.await.is_err());
    bank.cancel(&f.registry, &f.page, token)?;
    assert!(control.running());
    assert!(matches!(
        bank.publish(&f.registry, prepared),
        Err("workspace.busy")
    ));
    assert!(matches!(f.reader.reserve(), Err("workspace.busy")));
    let reader = f.reader.clone();
    let mut shutdown = tokio::spawn(async move { reader.shutdown().await });
    assert!(
        tokio::time::timeout(Duration::from_millis(40), &mut shutdown)
            .await
            .is_err()
    );
    drop(release);
    tokio::time::timeout(Duration::from_secs(5), shutdown).await???;
    assert!(!control.running());
    let other = Arc::new(ProfileReader::new());
    assert!(
        other
            .hosts(f.registry.clone(), f.page.clone(), f.database())
            .await?
            .records()
            .is_empty()
    );
    other.shutdown().await?;
    f.cleanup().await
}
