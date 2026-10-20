use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type Fixture = (
    tempfile::TempDir,
    Arc<KeymapOwner>,
    ProductDocuments,
    WindowToken,
    Arc<PageLaunchScope>,
);
fn fixture() -> std::result::Result<Fixture, Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let documents = ProductDocuments::new(registry.clone());
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    Ok((
        directory,
        KeymapOwner::new(registry, Handle::current()),
        documents,
        window,
        page,
    ))
}
fn change() -> Change {
    Change::Set {
        action: KeymapAction::Settings,
        keys: vec!["ctrl+shift+s".into()],
        when: "workspaceFocus && !editableFocus && !modalOpen".into(),
    }
}
#[tokio::test]
async fn actual_private_source_requires_confirmation_and_single_use_edit_consent() -> TestResult {
    let _serial = KEYMAP_TESTS.lock().await;
    let (directory, owner, _documents, _window, page) = fixture()?;
    let initial = owner.read(page.clone(), directory.path().into()).await?;
    assert!(!initial.confirmed);
    let defaults = pulse_store::keymap::KeymapPatch::parse("schema_version = 1\n")?.resolve()?;
    assert_eq!(initial.values, defaults);
    assert!(matches!(
        owner
            .save(page.clone(), initial.token, change(), true)
            .await,
        Err("workspace.denied")
    ));
    let confirmed = owner.confirm(page.clone(), initial.token).await?;
    assert!(confirmed.confirmed);
    assert_ne!(confirmed.token, initial.token);
    assert!(matches!(
        owner.confirm(page.clone(), initial.token).await,
        Err("workspace.denied")
    ));
    assert!(matches!(
        owner
            .save(page.clone(), confirmed.token, change(), false)
            .await,
        Err("workspace.keymap_rewrite_confirmation_required")
    ));
    let saved = owner
        .save(page.clone(), confirmed.token, change(), true)
        .await?;
    assert!(!saved.confirmed);
    assert!(
        saved
            .values
            .bindings
            .iter()
            .any(|binding| binding.action == KeymapAction::Settings)
    );
    assert!(matches!(
        owner
            .save(page.clone(), confirmed.token, change(), true)
            .await,
        Err("workspace.denied")
    ));
    let active = owner.confirm(page.clone(), saved.token).await?;
    let restored = owner
        .save(
            page,
            active.token,
            Change::Restore {
                action: KeymapAction::Settings,
            },
            true,
        )
        .await?;
    assert_eq!(restored.values, defaults);
    owner.shutdown().await?;
    let file = KeymapFile::open_private(directory.path().into())?;
    assert_eq!(file.load()?.projection()?, defaults);
    Ok(())
}
#[tokio::test]
async fn navigation_foreign_page_and_changed_directory_cannot_upgrade_old_tickets() -> TestResult {
    let _serial = KEYMAP_TESTS.lock().await;
    let (directory, owner, documents, window, page) = fixture()?;
    let read = owner.read(page.clone(), directory.path().into()).await?;
    let (_foreign_dir, _foreign_owner, _foreign_docs, _foreign_window, foreign) = fixture()?;
    assert!(matches!(
        owner.confirm(foreign, read.token).await,
        Err("workspace.denied")
    ));
    documents.started(window)?;
    owner.retire();
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let next = documents.current(window, nonce)?;
    assert!(matches!(
        owner.confirm(page, read.token).await,
        Err("workspace.denied")
    ));
    assert!(matches!(
        owner.confirm(next.clone(), read.token).await,
        Err("workspace.denied")
    ));
    let now = owner.read(next.clone(), directory.path().into()).await?;
    owner.confirm(next.clone(), now.token).await?;
    let other = tempfile::tempdir()?;
    assert!(matches!(
        owner.read(next, other.path().into()).await,
        Err("workspace.denied")
    ));
    assert!(
        owner
            .source
            .lock()
            .map_err(|_| "fixture.poison")?
            .as_ref()
            .is_some_and(|source| source.io.handle().is_open())
    );
    owner.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn source_edit_and_expired_capture_reject_without_overwriting_newer_bytes() -> TestResult {
    let _serial = KEYMAP_TESTS.lock().await;
    let (directory, owner, _documents, _window, page) = fixture()?;
    let initial = owner.read(page.clone(), directory.path().into()).await?;
    owner.pending.expire_for_test()?;
    assert!(matches!(
        owner.confirm(page.clone(), initial.token).await,
        Err("workspace.denied")
    ));
    let fresh = owner.read(page.clone(), directory.path().into()).await?;
    let accepted = owner.confirm(page.clone(), fresh.token).await?;
    fs::write(
        directory.path().join("keymap.toml"),
        "schema_version = 1\n# controlled external edit\n",
    )?;
    fs::set_permissions(
        directory.path().join("keymap.toml"),
        fs::Permissions::from_mode(0o600),
    )?;
    assert!(matches!(
        owner.save(page, accepted.token, change(), true).await,
        Err("workspace.keymap_stale")
    ));
    assert_eq!(
        fs::read_to_string(directory.path().join("keymap.toml"))?,
        "schema_version = 1\n# controlled external edit\n"
    );
    owner.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn dropped_rpc_and_cancelled_shutdown_observer_cannot_release_native_join() -> TestResult {
    let _serial = KEYMAP_TESTS.lock().await;
    let (directory, owner, _documents, _window, page) = fixture()?;
    let read = owner.read(page.clone(), directory.path().into()).await?;
    let io = owner
        .source
        .lock()
        .map_err(|_| "fixture.poison")?
        .as_ref()
        .ok_or("fixture.source")?
        .io
        .clone();
    let snapshot = io.handle().begin_load()?.ready().await?;
    let (start, started) = oneshot::channel();
    let start = Mutex::new(Some(start));
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let wait = Mutex::new(wait);
    let working = owner.clone();
    let original = page.clone();
    let rpc = tokio::spawn(async move {
        working
            .run(original.clone(), move |bank, cancelled| async move {
                let checking = bank.clone();
                let context = original.clone();
                let result =
                    io.handle()
                        .begin_confirm_retaining(
                            snapshot,
                            move || {
                                if let Ok(mut sender) = start.lock()
                                    && let Some(sender) = sender.take()
                                {
                                    let _sent = sender.send(());
                                }
                                if !wait.lock().is_ok_and(|wait| {
                                    wait.recv_timeout(Duration::from_secs(5)).is_ok()
                                }) {
                                    return false;
                                }
                                checking.check(&context, &cancelled).is_ok()
                            },
                            original.clone(),
                        )
                        .map_err(io_error)?
                        .ready()
                        .await
                        .map_err(io_error)?;
                bank.publish(original, result, true, &AtomicBool::new(false))
            })
            .await
    });
    tokio::time::timeout(Duration::from_secs(5), started).await??;
    rpc.abort();
    let _cancelled = rpc.await;
    assert_eq!(owner.credit.available_permits(), 0);
    let observer = owner.clone();
    let first = tokio::spawn(async move { observer.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!first.is_finished());
    first.abort();
    let _cancelled = first.await;
    let observer = owner.clone();
    let second = tokio::spawn(async move { observer.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!second.is_finished());
    release.send(())?;
    tokio::time::timeout(Duration::from_secs(5), second).await???;
    assert!(matches!(
        owner.confirm(page, read.token).await,
        Err("workspace.denied")
    ));
    assert!(owner.source.lock().map_err(|_| "fixture.poison")?.is_none());
    Ok(())
}
#[tokio::test]
async fn explicit_current_page_cancel_retires_tickets_without_closing_shared_source() -> TestResult
{
    let _serial = KEYMAP_TESTS.lock().await;
    let (directory, owner, _documents, _window, page) = fixture()?;
    let pending = owner.read(page.clone(), directory.path().into()).await?;
    let (_foreign_dir, _foreign_owner, _foreign_docs, _foreign_window, foreign) = fixture()?;
    assert!(matches!(
        owner.cancel_page(&foreign),
        Err("workspace.denied")
    ));
    let accepted = owner.confirm(page.clone(), pending.token).await?;
    owner.cancel_page(&page)?;
    assert!(matches!(
        owner
            .save(page.clone(), accepted.token, change(), true)
            .await,
        Err("workspace.denied")
    ));
    assert!(
        owner
            .source
            .lock()
            .map_err(|_| "fixture.poison")?
            .as_ref()
            .is_some_and(|source| source.io.handle().is_open())
    );
    let fresh = owner.read(page.clone(), directory.path().into()).await?;
    owner.cancel_page(&page)?;
    assert!(matches!(
        owner.confirm(page, fresh.token).await,
        Err("workspace.denied")
    ));
    owner.shutdown().await?;
    Ok(())
}

#[test]
fn input_rejects_unknown_actions_fields_and_noncanonical_tokens() {
    for json in [
        r#"{"kind":"set","action":"exec","keys":[],"when":""}"#,
        r#"{"kind":"restore","action":"settings","path":"/bin/sh"}"#,
    ] {
        assert!(serde_json::from_str::<Change>(json).is_err());
    }
    assert!(token("00000000-0000-0000-0000-000000000000").is_err());
    assert!(token("AAAAAAAA-AAAA-4AAA-8AAA-AAAAAAAAAAAA").is_err());
}
