use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture() -> TestResultFixture {
    let directory = tempfile::Builder::new()
        .prefix("pt-page-config.")
        .tempdir_in(std::fs::canonicalize(std::env::temp_dir())?)?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    fs::create_dir(directory.path().join("parts"))?;
    fs::set_permissions(
        directory.path().join("parts"),
        fs::Permissions::from_mode(0o700),
    )?;
    write(
        &directory.path().join("config"),
        b"Include parts/a.conf\nHost prod\n IdentityFile key-that-does-not-exist\n",
    )?;
    write(
        &directory.path().join("parts/a.conf"),
        b"Host prod\n HostName example.test\n User alice\n Port 2222\n",
    )?;
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
        registry,
        documents,
        window,
        page,
        reader: Arc::new(ProfileReader::new()),
    })
}
type TestResultFixture = std::result::Result<Fixture, Box<dyn std::error::Error>>;
struct Fixture {
    directory: tempfile::TempDir,
    registry: SessionRegistry,
    documents: ProductDocuments,
    window: pulse_app::registry::WindowToken,
    page: Arc<PageLaunchScope>,
    reader: Arc<ProfileReader>,
}
impl Fixture {
    fn request(&self) -> ConfigReadRequest {
        ConfigReadRequest {
            directory: self.directory.path().to_owned(),
            includes: Vec::new(),
            alias: "prod".into(),
            local_user: "local".into(),
        }
    }
    async fn capture(&self) -> Result<Arc<PageConfigCapture>> {
        self.reader
            .capture_config(self.registry.clone(), self.page.clone(), self.request())
            .await
    }
    async fn cleanup(self) -> TestResult {
        self.reader.shutdown().await?;
        self.registry.shutdown()?;
        self.directory.close()?;
        Ok(())
    }
}
fn write(path: &std::path::Path, bytes: &[u8]) -> TestResult {
    fs::write(path, bytes)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok(())
}
#[tokio::test]
async fn config_page_exact_metadata_and_external_change_rejection_without_key_or_database_read()
-> TestResult {
    let f = fixture()?;
    let capture = f.capture().await?;
    assert_eq!(capture.resolved().target().host(), "example.test");
    assert_eq!(capture.resolved().target().port(), 2222);
    assert_eq!(capture.resolved().user(), "alice");
    assert_eq!(
        capture.resolved().identity_files(),
        &["key-that-does-not-exist"]
    );
    f.reader.confirm_config(capture.clone()).await?;
    write(
        &f.directory.path().join("parts/a.conf"),
        b"Host prod\n HostName other.test\n",
    )?;
    assert_eq!(
        f.reader.confirm_config(capture).await,
        Err("workspace.config_conflict")
    );
    assert!(f.reader.store.get().is_none());
    assert!(!f.directory.path().join("key-that-does-not-exist").exists());
    f.cleanup().await
}
#[tokio::test]
async fn config_page_cross_reader_and_navigation_cannot_upgrade_old_capture() -> TestResult {
    let f = fixture()?;
    let capture = f.capture().await?;
    let other = Arc::new(ProfileReader::new());
    assert_eq!(
        other.confirm_config(capture.clone()).await,
        Err("workspace.denied")
    );
    other.shutdown().await?;
    f.documents.started(f.window)?;
    assert_eq!(
        f.reader.confirm_config(capture).await,
        Err("workspace.denied")
    );
    f.cleanup().await
}
#[tokio::test]
async fn config_page_one_use_ticket_and_expiry_bind_to_original_confirmed_document() -> TestResult {
    let f = fixture()?;
    let bank =
        super::super::super::metadata_captures::MetadataCaptures::new("fixture.capture_failed");
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    let capture = bank.take(&f.registry, &f.page, ticket)?;
    assert!(bank.take(&f.registry, &f.page, ticket).is_err());
    f.reader.confirm_config(capture).await?;
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    bank.expire_for_test()?;
    assert!(bank.take(&f.registry, &f.page, ticket).is_err());
    bank.retire();
    f.cleanup().await
}
#[tokio::test]
async fn config_page_queued_read_revokes_before_filesystem_work() -> TestResult {
    let f = fixture()?;
    let held = f.reader.reserve()?;
    let mut pending = Box::pin(f.capture());
    std::future::poll_fn(|cx| {
        use std::future::Future;
        assert!(pending.as_mut().poll(cx).is_pending());
        std::task::Poll::Ready(())
    })
    .await;
    f.documents.started(f.window)?;
    f.reader.retire_read_waiters();
    assert_eq!(pending.await.err(), Some("workspace.denied"));
    assert_eq!(f.reader.credit.available_permits(), 0);
    drop(held);
    f.cleanup().await
}
async fn active_read(cancel: bool) -> TestResult {
    use std::time::Duration;
    let f = fixture()?;
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let reader = f.reader.clone();
    let registry = f.registry.clone();
    let page = f.page.clone();
    let request = f.request();
    let pending = tokio::spawn(async move {
        reader
            .capture_config_inner(registry, page, request, move || {
                let _ = started.send(());
                released
                    .recv_timeout(Duration::from_secs(5))
                    .map_err(|_| "fixture.release")?;
                Ok(())
            })
            .await
    });
    let result: TestResult = async {
        tokio::time::timeout(Duration::from_secs(3), entered).await??;
        if f.reader.credit.available_permits() != 0 {
            return Err("fixture.credit_not_held".into());
        }
        if cancel {
            pending.abort();
            f.reader.close();
        } else {
            f.documents.started(f.window)?;
        }
        if cancel {
            let mut shutdown = Box::pin(f.reader.shutdown());
            std::future::poll_fn(|cx| {
                use std::future::Future;
                std::task::Poll::Ready(if shutdown.as_mut().poll(cx).is_pending() {
                    Ok(())
                } else {
                    Err("fixture.shutdown_finished_early")
                })
            })
            .await?;
            if f.reader.credit.available_permits() != 0 {
                return Err("fixture.cancel_released_credit".into());
            }
        }
        Ok(())
    }
    .await;
    // Release the owned native closure on every assertion/timeout path before fixture cleanup.
    let _ = release.send(());
    let joined = tokio::time::timeout(Duration::from_secs(5), pending).await;
    let drained = tokio::time::timeout(Duration::from_secs(5), f.reader.shutdown()).await;
    let verification: TestResult = (|| {
        result?;
        let joined = joined?;
        if cancel {
            if joined.is_ok() {
                return Err("fixture.waiter_not_cancelled".into());
            }
        } else if joined?.err() != Some("workspace.denied") {
            return Err("fixture.stale_read_published".into());
        }
        drained??;
        if f.reader.credit.available_permits() != 1 {
            return Err("fixture.credit_not_returned".into());
        }
        Ok(())
    })();
    f.cleanup().await?;
    verification
}
#[tokio::test]
async fn config_page_late_native_read_cannot_publish_after_navigation() -> TestResult {
    active_read(false).await
}
#[tokio::test]
async fn config_page_cancelled_native_read_keeps_credit_and_exit_barrier_until_actual_exit()
-> TestResult {
    active_read(true).await
}

#[tokio::test]
async fn config_page_discovery_new_member_conflicts_and_fresh_preview_observes_new_value()
-> TestResult {
    let f = fixture()?;
    write(
        &f.directory.path().join("config"),
        b"Host prod\nHostName demo.example\nInclude parts/*.conf\n",
    )?;
    let capture = f.capture().await?;
    assert_eq!(capture.resolved().user(), "alice");
    f.reader.confirm_config(capture.clone()).await?;
    write(&f.directory.path().join("parts/00.conf"), b"User newest\n")?;
    assert_eq!(
        f.reader.confirm_config(capture).await,
        Err("workspace.config_conflict")
    );
    let fresh = f.capture().await?;
    assert_eq!(fresh.resolved().user(), "newest");
    f.reader.confirm_config(fresh.clone()).await?;
    write(
        &f.directory.path().join("parts/a.conf"),
        b"User externally_changed\n",
    )?;
    assert_eq!(
        f.reader.confirm_config(fresh).await,
        Err("workspace.config_conflict")
    );
    f.cleanup().await
}

#[tokio::test]
async fn config_page_status_borrow_preserves_single_use_and_original_expiry() -> TestResult {
    let f = fixture()?;
    let bank =
        super::super::super::metadata_captures::MetadataCaptures::new("fixture.capture_failed");
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    for _ in 0..2 {
        f.reader
            .confirm_config(bank.borrow(&f.registry, &f.page, ticket)?)
            .await?;
    }
    f.reader
        .confirm_config(bank.take(&f.registry, &f.page, ticket)?)
        .await?;
    assert!(bank.borrow(&f.registry, &f.page, ticket).is_err());
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    let pending = bank.borrow(&f.registry, &f.page, ticket)?;
    bank.expire_for_test()?;
    f.reader.confirm_config(pending).await?;
    assert!(bank.borrow(&f.registry, &f.page, ticket).is_err());
    assert!(bank.take(&f.registry, &f.page, ticket).is_err());
    f.cleanup().await
}
#[tokio::test]
async fn config_page_status_replacement_and_navigation_reject_late_old_borrow() -> TestResult {
    let f = fixture()?;
    let bank =
        super::super::super::metadata_captures::MetadataCaptures::new("fixture.capture_failed");
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    let pending = bank.borrow(&f.registry, &f.page, ticket)?;
    let fresh = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    f.reader.confirm_config(pending).await?;
    assert!(bank.borrow(&f.registry, &f.page, ticket).is_err());
    assert!(
        bank.borrow(&f.registry, &f.page, uuid::Uuid::new_v4())
            .is_err()
    );
    f.documents.started(f.window)?;
    assert!(bank.borrow(&f.registry, &f.page, fresh).is_err());
    f.cleanup().await
}
#[tokio::test]
async fn config_page_status_change_preserves_explicit_confirmation_ticket() -> TestResult {
    let f = fixture()?;
    let bank =
        super::super::super::metadata_captures::MetadataCaptures::new("fixture.capture_failed");
    let ticket = bank.publish(&f.registry, f.page.clone(), f.capture().await?)?;
    write(
        &f.directory.path().join("parts/a.conf"),
        b"User changed_without_explicit_confirmation\n",
    )?;
    assert_eq!(
        f.reader
            .confirm_config(bank.borrow(&f.registry, &f.page, ticket)?)
            .await,
        Err("workspace.config_conflict")
    );
    assert_eq!(
        f.reader
            .confirm_config(bank.take(&f.registry, &f.page, ticket)?)
            .await,
        Err("workspace.config_conflict")
    );
    f.cleanup().await
}
#[tokio::test]
async fn config_page_active_status_cancellation_keeps_credit_until_native_exit() -> TestResult {
    let f = fixture()?;
    let capture = f.capture().await?;
    let (started, entered) = tokio::sync::oneshot::channel();
    let (release, released) = std::sync::mpsc::channel();
    let reader = f.reader.clone();
    let pending = tokio::spawn(async move {
        reader
            .confirm_config_inner(capture, move || {
                let _ = started.send(());
                released
                    .recv_timeout(std::time::Duration::from_secs(5))
                    .map_err(|_| "fixture.timeout")
            })
            .await
    });
    entered.await?;
    pending.abort();
    let _ = pending.await;
    let credit_retained = f.reader.credit.available_permits() == 0;
    let owner = f.reader.clone();
    let shutdown = tokio::spawn(async move { owner.shutdown().await });
    tokio::task::yield_now().await;
    let shutdown_waits = !shutdown.is_finished();
    release.send(())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), shutdown).await???;
    f.cleanup().await?;
    assert!(credit_retained);
    assert!(shutdown_waits);
    Ok(())
}
