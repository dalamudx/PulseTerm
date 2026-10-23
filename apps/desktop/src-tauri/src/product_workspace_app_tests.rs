#![allow(clippy::unwrap_used)]
use super::*;
use std::ffi::OsString;
use std::{fs, os::unix::fs::PermissionsExt};
use tokio::time::{Duration, timeout};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;

pub(crate) fn application() -> Arc<Application> {
    super::test_application()
}
pub(super) fn window(
    application: &Arc<Application>,
) -> (Workspace, Uuid, Arc<pulse_app::registry::PageLaunchScope>) {
    let owner = application.registry.open_window().unwrap();
    application.documents.install(owner).unwrap();
    let reader = Arc::new(profiles::ProfileReader::with_storage(
        application.profile_storage.clone(),
    ));
    let state = Workspace::from_application(application.clone(), owner, reader).unwrap();
    application.documents.started(owner).unwrap();
    let nonce = application.documents.finished(owner).unwrap();
    application.documents.confirm(owner, nonce).unwrap();
    let page = application.documents.current(owner, nonce).unwrap();
    (state, nonce, page)
}

#[tokio::test]
async fn foreign_profile_service_cannot_become_an_application_window() -> TestResult {
    let application = application();
    let owner = application.registry.open_window()?;
    application.documents.install(owner)?;
    assert!(
        Workspace::from_application(
            application.clone(),
            owner,
            Arc::new(profiles::ProfileReader::new())
        )
        .is_err()
    );
    application.documents.remove(owner)?;
    application.registry.close_window(owner)?;
    timeout(
        Duration::from_secs(2),
        application.profile_storage.shutdown(),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn retiring_one_native_context_revokes_its_vault_operation_but_preserves_other_page()
-> TestResult {
    let application = application();
    let (a, nonce_a, page_a) = window(&application);
    let (b, nonce_b, page_b) = window(&application);
    let operation_a = a.0.vault.begin(page_a.clone())?;
    let operation_b = b.0.vault.begin(page_b.clone())?;
    let valid_a = operation_a.authorization();
    let valid_b = operation_b.authorization();
    assert!(valid_a() && valid_b());
    a.retire_window()?;
    assert!(!valid_a() && valid_b());
    assert!(application.documents.current(a.0.owner, nonce_a).is_err());
    assert!(Arc::ptr_eq(
        &application.documents.current(b.0.owner, nonce_b)?,
        &page_b
    ));
    assert!(Arc::ptr_eq(&a.0.sessions, &b.0.sessions));
    assert!(matches!(application.exit.request(), exit::Action::Begin));
    b.retire_window()?;
    drop((valid_a, valid_b, operation_a, operation_b));
    timeout(Duration::from_secs(2), a.0.profiles.drain_window()).await??;
    timeout(Duration::from_secs(2), b.0.profiles.drain_window()).await??;
    timeout(
        Duration::from_secs(2),
        application.profile_storage.shutdown(),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn native_context_retirement_keeps_other_reader_and_same_real_sql_worker_usable() -> TestResult
{
    let temp = tempfile::tempdir()?;
    let application = application();
    let (a, _, page_a) = window(&application);
    let (b, _, page_b) = window(&application);
    let directory = temp.path().join("state");
    let before =
        a.0.profiles
            .list(
                application.registry.clone(),
                page_a,
                directory.clone(),
                OsString::from("/bin"),
                None,
            )
            .await?;
    a.retire_window()?;
    timeout(Duration::from_secs(2), a.0.profiles.drain_window()).await??;
    let after =
        b.0.profiles
            .list(
                application.registry.clone(),
                page_b,
                directory,
                OsString::from("/bin"),
                None,
            )
            .await?;
    assert_eq!(before.len(), after.len());
    assert!(Arc::ptr_eq(
        &a.0.profiles.shared_storage(),
        &b.0.profiles.shared_storage()
    ));
    b.retire_window()?;
    timeout(Duration::from_secs(2), b.0.profiles.drain_window()).await??;
    timeout(
        Duration::from_secs(2),
        application.profile_storage.shutdown(),
    )
    .await??;
    Ok(())
}

#[tokio::test]
async fn independent_keymap_pages_reject_stale_save_and_survive_other_window_retirement()
-> TestResult {
    let _serial = KEYMAP_TESTS.lock().await;
    let application = application();
    let (a, _, page_a) = window(&application);
    let (b, _, page_b) = window(&application);
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let capture_a =
        a.0.keymaps
            .read(page_a.clone(), directory.path().into())
            .await?;
    let capture_b =
        b.0.keymaps
            .read(page_b.clone(), directory.path().into())
            .await?;
    let confirmed_a = a.0.keymaps.confirm(page_a.clone(), capture_a.token).await?;
    let confirmed_b = b.0.keymaps.confirm(page_b.clone(), capture_b.token).await?;
    a.0.keymaps
        .save(
            page_a,
            confirmed_a.token,
            keymap::Change::Set {
                action: pulse_store::keymap::KeymapAction::Settings,
                keys: vec!["ctrl+shift+s".into()],
                when: String::new(),
            },
            true,
        )
        .await?;
    assert_eq!(
        b.0.keymaps
            .save(
                page_b.clone(),
                confirmed_b.token,
                keymap::Change::Restore {
                    action: pulse_store::keymap::KeymapAction::Settings,
                },
                true
            )
            .await
            .err(),
        Some("workspace.keymap_stale")
    );
    a.retire_window()?;
    a.0.keymaps.shutdown().await?;
    let fresh =
        b.0.keymaps
            .read(page_b.clone(), directory.path().into())
            .await?;
    let confirmed = b.0.keymaps.confirm(page_b.clone(), fresh.token).await?;
    b.0.keymaps
        .save(
            page_b,
            confirmed.token,
            keymap::Change::Restore {
                action: pulse_store::keymap::KeymapAction::Settings,
            },
            true,
        )
        .await?;
    b.retire_window()?;
    b.0.keymaps.shutdown().await?;
    application.profile_storage.shutdown().await?;
    application.settings_io.shutdown().await?;
    Ok(())
}

#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
fn cli_action(
    application: &Application,
    page: &Arc<pulse_app::registry::PageLaunchScope>,
) -> pulse_app::window_action::PreparedWindowAction {
    pulse_app::window_action::PreparedWindowAction::prepare(
        &capabilities::catalogue(cfg!(feature = "workspace-ssh")).unwrap(),
        &application.registry,
        page.clone(),
        "workspace.new-tab",
        pulse_cap::Surface::Cli,
        serde_json::json!({"profile":"sh"}),
    )
    .unwrap()
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[tokio::test]
async fn simultaneous_window_cli_confirmations_and_retirement_cannot_consume_foreign_bank()
-> TestResult {
    let application = application();
    let (a, _, page_a) = window(&application);
    let (b, _, page_b) = window(&application);
    assert!(!Arc::ptr_eq(&a.0.cli_approvals, &b.0.cli_approvals));
    let mut first = a.0.cli_approvals.reserve(
        page_a.clone(),
        tokio_util::sync::CancellationToken::new(),
        cli_action(&application, &page_a),
    )?;
    let mut second = b.0.cli_approvals.reserve(
        page_b.clone(),
        tokio_util::sync::CancellationToken::new(),
        cli_action(&application, &page_b),
    )?;
    assert!(cli::answer(&b, &page_b, &first.id.to_string(), true).is_err());
    assert!(cli::answer(&a, &page_b, &first.id.to_string(), true).is_err());
    a.retire_window()?;
    assert!((&mut first.answer).await.is_err());
    cli::answer(&b, &page_b, &second.id.to_string(), true)?;
    let action = (&mut second.answer).await??;
    assert_eq!(
        action.take_args(&page_b, "workspace.new-tab")?["profile"],
        "sh"
    );
    application.window_drains.shutdown().await?;
    application.profile_storage.shutdown().await?;
    application.settings_io.shutdown().await?;
    Ok(())
}
#[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
#[tokio::test]
async fn navigation_and_old_cli_reservation_drop_cannot_revoke_other_or_replacement_bank()
-> TestResult {
    let application = application();
    let (a, _, old_page) = window(&application);
    let (b, _, page_b) = window(&application);
    let first = a.0.cli_approvals.reserve(
        old_page.clone(),
        tokio_util::sync::CancellationToken::new(),
        cli_action(&application, &old_page),
    )?;
    let mut other = b.0.cli_approvals.reserve(
        page_b.clone(),
        tokio_util::sync::CancellationToken::new(),
        cli_action(&application, &page_b),
    )?;
    application.documents.started(a.0.owner)?;
    cli::retire(&a);
    let nonce = application.documents.finished(a.0.owner)?;
    application.documents.confirm(a.0.owner, nonce)?;
    let fresh_page = application.documents.current(a.0.owner, nonce)?;
    let mut fresh = a.0.cli_approvals.reserve(
        fresh_page.clone(),
        tokio_util::sync::CancellationToken::new(),
        cli_action(&application, &fresh_page),
    )?;
    drop(first);
    assert!(cli::answer(&a, &old_page, &fresh.id.to_string(), true).is_err());
    cli::answer(&a, &fresh_page, &fresh.id.to_string(), true)?;
    (&mut fresh.answer)
        .await??
        .take_args(&fresh_page, "workspace.new-tab")?;
    cli::answer(&b, &page_b, &other.id.to_string(), true)?;
    (&mut other.answer)
        .await??
        .take_args(&page_b, "workspace.new-tab")?;
    application.window_drains.shutdown().await?;
    application.profile_storage.shutdown().await?;
    application.settings_io.shutdown().await?;
    Ok(())
}
