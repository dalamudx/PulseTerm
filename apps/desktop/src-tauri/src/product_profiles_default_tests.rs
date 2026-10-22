use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{fs, os::unix::fs::PermissionsExt};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type PageFixture = (
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
);
fn page() -> std::result::Result<PageFixture, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let scope = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, scope))
}
fn fixture() -> std::io::Result<(tempfile::TempDir, PathBuf, OsString)> {
    let root = tempfile::tempdir()?;
    let bin = root.path().join("bin");
    fs::create_dir(&bin)?;
    for name in ["bash", "sh"] {
        let path = bin.join(name);
        fs::write(&path, b"not executed")?;
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    }
    let dir = root.path().join("state");
    Ok((root, dir, bin.into_os_string()))
}

#[cfg(all(feature = "workspace-cli", target_os = "linux"))]
#[tokio::test]
async fn cli_default_reads_do_not_replace_gui_ticket_or_choose_an_unsaved_shell() -> TestResult {
    let (_root, dir, path) = fixture()?;
    let (registry, documents, owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(
        reader
            .default_selection(
                registry.clone(),
                page.clone(),
                dir.clone(),
                path.clone(),
                None
            )
            .await?,
        None
    );
    assert_eq!(
        reader
            .default_ticket
            .lock()
            .map_err(|_| "poisoned")?
            .as_ref()
            .map(|ticket| ticket.id),
        Some(offer.ticket)
    );
    reader
        .default_save(
            registry.clone(),
            page.clone(),
            (dir.clone(), path.clone(), None),
            offer.ticket,
            Some(AutomaticShell::Sh),
        )
        .await?;
    assert_eq!(
        reader
            .default_selection(
                registry.clone(),
                page.clone(),
                dir.clone(),
                path.clone(),
                None
            )
            .await?,
        Some(pulse_store::profile_default::DefaultProfileSelection::Automatic(AutomaticShell::Sh))
    );
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    reader
        .default_selection(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    reader
        .default_save(
            registry.clone(),
            page.clone(),
            (dir.clone(), path.clone(), None),
            offer.ticket,
            None,
        )
        .await?;
    assert_eq!(
        reader
            .default_selection(
                registry.clone(),
                page.clone(),
                dir.clone(),
                path.clone(),
                None
            )
            .await?,
        None
    );
    documents.started(owner)?;
    assert!(
        reader
            .default_selection(registry, page, dir, path, None)
            .await
            .is_err()
    );
    reader.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn default_metadata_save_is_single_use_and_missing_hidden_defaults_do_not_fallback()
-> TestResult {
    let (_root, dir, path) = fixture()?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(offer.selected, None);
    assert!(!offer.available);
    assert_eq!(offer.offered.len(), 2);
    reader
        .default_save(
            registry.clone(),
            page.clone(),
            (dir.clone(), path.clone(), None),
            offer.ticket,
            Some(AutomaticShell::Bash),
        )
        .await?;
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                offer.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(offer.selected, Some(AutomaticShell::Bash));
    assert!(offer.available);
    let (external, task) = StateStore::open(dir.join("state.db")).await?;
    let snapshot = external.load_default_profile().await?;
    let only_sh = [pulse_pty::profiles::ShellCandidate::new(
        pulse_pty::profiles::ShellKind::Sh,
        PathBuf::from(&path).join("sh"),
    )?];
    assert_eq!(
        pulse_app::profile_state::resolve_default_profile(&snapshot, &only_sh),
        Err(pulse_pty::profiles::ProfileError::Unavailable)
    );
    external
        .edit_auto_profiles(pulse_store::profiles::AutoProfileEdit::Visibility {
            shell: AutomaticShell::Bash,
            hidden: true,
        })
        .await?;
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(offer.selected, Some(AutomaticShell::Bash));
    assert!(!offer.available);
    assert_eq!(offer.offered, [AutomaticShell::Sh]);
    reader
        .default_save(
            registry.clone(),
            page.clone(),
            (dir.clone(), path.clone(), None),
            offer.ticket,
            None,
        )
        .await?;
    let offer = reader.default_read(registry, page, dir, path, None).await?;
    assert_eq!(offer.selected, None);
    reader.shutdown().await?;
    drop(external);
    task.join().await?;
    Ok(())
}
#[tokio::test]
async fn old_replaced_cancelled_expired_and_foreign_page_tickets_cannot_write() -> TestResult {
    let (_root, dir, path) = fixture()?;
    let (registry, documents, owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let old = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    let new = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    reader.default_cancel(&registry, &page, old.ticket)?; // cannot cancel the replacement
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                old.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    reader.default_cancel(&registry, &page, new.ticket)?;
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                new.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    let expired = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    {
        let mut bank = reader.default_ticket.lock().map_err(|_| "fixture mutex")?;
        bank.as_mut().ok_or("fixture missing ticket")?.deadline = Instant::now();
    }
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                expired.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    let old = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let next = documents.current(owner, nonce)?;
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                next.clone(),
                (dir.clone(), path.clone(), None),
                old.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page,
                (dir.clone(), path.clone(), None),
                old.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.denied")
    );
    let fresh = reader.default_read(registry, next, dir, path, None).await?;
    assert_eq!(fresh.selected, None);
    reader.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn changed_visibility_and_discovery_reject_save_against_original_source() -> TestResult {
    let (_root, dir, path) = fixture()?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    let (external, task) = StateStore::open(dir.join("state.db")).await?;
    external
        .edit_auto_profiles(pulse_store::profiles::AutoProfileEdit::Visibility {
            shell: AutomaticShell::Bash,
            hidden: true,
        })
        .await?;
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                offer.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.profiles_conflict")
    );
    let offer = reader
        .default_read(
            registry.clone(),
            page.clone(),
            dir.clone(),
            path.clone(),
            None,
        )
        .await?;
    assert_eq!(offer.offered, [AutomaticShell::Sh]);
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                offer.ticket,
                Some(AutomaticShell::Bash)
            )
            .await,
        Err("workspace.denied")
    );
    external
        .edit_auto_profiles(pulse_store::profiles::AutoProfileEdit::Visibility {
            shell: AutomaticShell::Sh,
            hidden: true,
        })
        .await?;
    assert_eq!(
        reader
            .default_save(
                registry.clone(),
                page.clone(),
                (dir.clone(), path.clone(), None),
                offer.ticket,
                Some(AutomaticShell::Sh)
            )
            .await,
        Err("workspace.profiles_conflict")
    );
    assert_eq!(external.load_default_profile().await?.shell(), None);
    reader.shutdown().await?;
    drop(external);
    task.join().await?;
    Ok(())
}
