use super::*;
use pulse_app::product_document::ProductDocuments;
use pulse_store::settings::SettingsSource;
use std::{fs, os::unix::fs::PermissionsExt, time::Duration};
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error>>;
fn page() -> TestResult<(
    SessionRegistry,
    ProductDocuments,
    WindowToken,
    Arc<PageLaunchScope>,
)> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let page = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, page))
}
#[tokio::test]
async fn keymap_current_source_confirmation_rejects_retired_page_and_foreign_registry() -> TestResult
{
    let _serial = KEYMAP_TESTS.lock().await;
    use pulse_store::{
        keymap_file::{KeymapFile, KeymapFileError},
        keymap_io::{KeymapIoError, KeymapIoOwner},
    };
    let dir = tempfile::tempdir()?;
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700))?;
    let (registry, documents, window, page) = page()?;
    let owner = KeymapIoOwner::new(
        KeymapFile::open_private(dir.path().into())?,
        tokio::runtime::Handle::current(),
    );
    let source = owner.handle().begin_load()?.ready().await?;
    let r = registry.clone();
    let original = page.clone();
    owner
        .handle()
        .begin_confirm_retaining(
            source.clone(),
            move || r.check_launch_page(&original).is_ok(),
            (),
        )?
        .ready()
        .await?;
    let foreign = SessionRegistry::new();
    let original = page.clone();
    assert!(matches!(
        owner
            .handle()
            .begin_confirm_retaining(
                source.clone(),
                move || foreign.check_launch_page(&original).is_ok(),
                ()
            )?
            .ready()
            .await,
        Err(KeymapIoError::File(KeymapFileError::Authorization))
    ));
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let new_page = documents.current(window, nonce)?;
    let r = registry.clone();
    let original = page;
    assert!(matches!(
        owner
            .handle()
            .begin_confirm_retaining(
                source.clone(),
                move || r.check_launch_page(&original).is_ok(),
                ()
            )?
            .ready()
            .await,
        Err(KeymapIoError::File(KeymapFileError::Authorization))
    ));
    let r = registry.clone();
    let current = owner.handle().begin_load()?.ready().await?;
    owner
        .handle()
        .begin_confirm_retaining(current, move || r.check_launch_page(&new_page).is_ok(), ())?
        .ready()
        .await?;
    owner.close();
    assert!(matches!(
        owner.handle().begin_load(),
        Err(KeymapIoError::Closed)
    ));
    Ok(())
}

#[tokio::test]
async fn keymap_inflight_source_confirmation_cannot_publish_after_actual_navigation() -> TestResult
{
    use pulse_store::{
        keymap_file::{KeymapFile, KeymapFileError},
        keymap_io::{KeymapIoError, KeymapIoOwner},
    };
    let _serial = KEYMAP_TESTS.lock().await;
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let (registry, documents, window, original_page) = page()?;
    let owner = KeymapIoOwner::new(
        KeymapFile::open_private(directory.path().into())?,
        tokio::runtime::Handle::current(),
    );
    let original = owner.handle().begin_load()?.ready().await?;
    let (start, started) = tokio::sync::oneshot::channel();
    let start = Mutex::new(Some(start));
    let (release, wait) = std::sync::mpsc::sync_channel(1);
    let wait = Mutex::new(wait);
    let original_registry = registry.clone();
    let request = owner.handle().begin_confirm_retaining(
        original,
        move || {
            if let Ok(mut sender) = start.lock()
                && let Some(sender) = sender.take()
            {
                let _sent = sender.send(());
            }
            if !wait
                .lock()
                .is_ok_and(|wait| wait.recv_timeout(Duration::from_secs(5)).is_ok())
            {
                return false;
            }
            original_registry.check_launch_page(&original_page).is_ok()
        },
        (),
    )?;
    tokio::time::timeout(Duration::from_secs(5), started).await??;
    documents.started(window)?;
    assert!(matches!(
        owner.handle().begin_load(),
        Err(KeymapIoError::Busy)
    ));
    assert!(owner.handle().is_open());
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let current_page = documents.current(window, nonce)?;
    release.send(())?;
    assert!(matches!(
        request.ready().await,
        Err(KeymapIoError::File(KeymapFileError::Authorization))
    ));
    let current = owner.handle().begin_load()?.ready().await?;
    let captured = registry;
    owner
        .handle()
        .begin_confirm_retaining(
            current,
            move || captured.check_launch_page(&current_page).is_ok(),
            (),
        )?
        .ready()
        .await?;
    owner.close();
    Ok(())
}

#[tokio::test]
async fn actual_fixed_file_defaults_overrides_reset_and_reopen() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("config");
    let (r, _d, _w, p) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let initial = owner.read(r.clone(), p.clone(), dir.clone()).await?;
    assert_eq!(initial.values.font_size.value, 14);
    assert_eq!(initial.values.font_size.source, SettingsSource::Default);
    let changed = owner
        .save(
            r.clone(),
            p.clone(),
            dir.clone(),
            initial.token,
            Change::FontSize(18),
        )
        .await?;
    assert_eq!(changed.values.font_size.value, 18);
    assert_eq!(changed.values.font_size.source, SettingsSource::User);
    let changed = owner
        .save(
            r.clone(),
            p.clone(),
            dir.clone(),
            changed.token,
            Change::CursorBlink(true),
        )
        .await?;
    let changed = owner
        .save(
            r.clone(),
            p.clone(),
            dir.clone(),
            changed.token,
            Change::ResetFontSize,
        )
        .await?;
    assert_eq!(changed.values.font_size.value, 14);
    assert_eq!(changed.values.font_size.source, SettingsSource::Default);
    assert!(changed.values.cursor_blink.value);
    owner.shutdown().await?;
    let reopened = Arc::new(SettingsOwner::new());
    let read = reopened.read(r, p, dir).await?;
    assert!(read.values.cursor_blink.value);
    assert_eq!(read.values.cursor_blink.source, SettingsSource::User);
    reopened.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn stale_foreign_owner_and_changed_directory_tokens_cannot_write() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("config");
    let (r, _d, _w, p) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let a = owner.read(r.clone(), p.clone(), dir.clone()).await?;
    let b = owner.read(r.clone(), p.clone(), dir.clone()).await?;
    assert!(matches!(
        owner
            .save(
                r.clone(),
                p.clone(),
                dir.clone(),
                a.token,
                Change::FontSize(20)
            )
            .await,
        Err("workspace.settings_stale")
    ));
    assert!(matches!(
        owner
            .read(r.clone(), p.clone(), temp.path().join("other"))
            .await,
        Err("workspace.denied")
    ));
    let foreign = Arc::new(SettingsOwner::new());
    let _ = foreign.read(r.clone(), p.clone(), dir.clone()).await?;
    assert!(matches!(
        foreign
            .save(
                r.clone(),
                p.clone(),
                dir.clone(),
                b.token,
                Change::FontSize(20)
            )
            .await,
        Err("workspace.settings_stale")
    ));
    let current = owner.read(r, p, dir).await?;
    assert_eq!(current.values.font_size.value, 14);
    owner.shutdown().await?;
    foreign.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn retired_page_cannot_read_save_or_upgrade_original_snapshot() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("config");
    let (r, d, w, p) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let captured = owner.read(r.clone(), p.clone(), dir.clone()).await?;
    d.started(w)?;
    assert!(matches!(
        owner
            .save(
                r.clone(),
                p.clone(),
                dir.clone(),
                captured.token,
                Change::FontSize(20)
            )
            .await,
        Err("workspace.denied")
    ));
    let nonce = d.finished(w)?;
    d.confirm(w, nonce)?;
    let fresh = d.current(w, nonce)?;
    assert!(matches!(
        owner
            .save(
                r.clone(),
                fresh.clone(),
                dir.clone(),
                captured.token,
                Change::FontSize(20)
            )
            .await,
        Err("workspace.settings_stale")
    ));
    assert!(matches!(
        owner.read(r.clone(), p, dir.clone()).await,
        Err("workspace.denied")
    ));
    let fresh = owner.read(r, fresh, dir).await?;
    assert_eq!(fresh.values.font_size.value, 14);
    owner.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn malformed_or_externally_changed_file_never_falls_back_or_overwrites() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("config");
    let (r, _d, _w, p) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let a = owner.read(r.clone(), p.clone(), dir.clone()).await?;
    fs::write(dir.join("settings.toml"), "[terminal]\nfont_size = 22\n")?;
    fs::set_permissions(dir.join("settings.toml"), fs::Permissions::from_mode(0o600))?;
    assert!(matches!(
        owner
            .save(
                r.clone(),
                p.clone(),
                dir.clone(),
                a.token,
                Change::FontSize(20)
            )
            .await,
        Err("workspace.settings_save_unconfirmed")
    ));
    assert!(fs::read_to_string(dir.join("settings.toml"))?.contains("22"));
    fs::write(dir.join("settings.toml"), "schema_version = 9\n")?;
    assert!(matches!(
        owner.read(r, p, dir).await,
        Err("workspace.settings_invalid")
    ));
    owner.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn cancelled_waiter_and_shutdown_retain_actual_blocking_credit() -> TestResult {
    let owner = Arc::new(SettingsOwner::new());
    let (started, receipt) = tokio::sync::oneshot::channel();
    let (release, held) = std::sync::mpsc::channel();
    let running = owner.clone();
    let waiter = tokio::spawn(async move {
        running
            .io(move |_| {
                started.send(()).map_err(|_| "fixture.sender")?;
                held.recv().map_err(|_| "fixture.receiver")?;
                Ok(())
            })
            .await
    });
    receipt.await?;
    waiter.abort();
    assert!(matches!(owner.reserve(), Err("workspace.busy")));
    let closing = owner.clone();
    let exit = tokio::spawn(async move { closing.shutdown().await });
    tokio::task::yield_now().await;
    assert!(!exit.is_finished());
    assert_eq!(owner.credit.available_permits(), 0);
    release.send(())?;
    tokio::time::timeout(Duration::from_secs(2), exit).await???;
    assert_eq!(owner.credit.available_permits(), 1);
    assert!(matches!(owner.reserve(), Err("workspace.denied")));
    Ok(())
}

#[tokio::test]
async fn ui_appearance_persists_with_original_page_conflict_and_reset() -> TestResult {
    use pulse_store::settings::UiAppearance;
    let temporary = tempfile::tempdir()?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))?;
    let dir = temporary.path().join("settings");
    let (registry, _documents, _window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let initial = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(initial.version, 15);
    assert_eq!(initial.values.ui_appearance.value, UiAppearance::Light);
    let changed = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            initial.token,
            Change::UiAppearance(UiAppearance::Dark),
        )
        .await?;
    assert_eq!(changed.values.ui_appearance.source, SettingsSource::User);
    assert_eq!(changed.values.ui_appearance.value, UiAppearance::Dark);
    assert_eq!(changed.values.font_size.value, 14);
    assert!(fs::read_to_string(dir.join("settings.toml"))?.contains("\"dark\""));
    let external = "# external edit\n[ui]\nappearance='system'\n[terminal]\nfont_size=18\n";
    fs::write(dir.join("settings.toml"), external)?;
    assert!(matches!(
        owner
            .save(
                registry.clone(),
                page.clone(),
                dir.clone(),
                changed.token,
                Change::UiAppearance(UiAppearance::Light)
            )
            .await,
        Err("workspace.settings_save_unconfirmed")
    ));
    assert_eq!(fs::read_to_string(dir.join("settings.toml"))?, external);
    let fresh = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(fresh.values.ui_appearance.value, UiAppearance::System);
    let reset = owner
        .save(
            registry,
            page,
            dir.clone(),
            fresh.token,
            Change::ResetUiAppearance,
        )
        .await?;
    assert_eq!(reset.values.ui_appearance.value, UiAppearance::Light);
    assert_eq!(reset.values.ui_appearance.source, SettingsSource::Default);
    assert_eq!(reset.values.font_size.value, 18);
    assert!(fs::read_to_string(dir.join("settings.toml"))?.contains("# external edit"));
    owner.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn terminal_preset_read_keeps_save_ticket_and_rejects_retired_page() -> TestResult {
    use pulse_store::settings::TerminalPreset;
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(
        owner
            .terminal_preset(registry.clone(), page.clone(), dir.clone())
            .await?,
        TerminalPreset::Classic
    );
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::TerminalPreset(TerminalPreset::Dracula),
        )
        .await?;
    assert_eq!(
        owner
            .terminal_preset(registry.clone(), page.clone(), dir.clone())
            .await?,
        TerminalPreset::Dracula
    );
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            saved.token,
            Change::ResetTerminalPreset,
        )
        .await?;
    assert_eq!(reset.values.terminal_preset.value, TerminalPreset::Classic);
    documents.started(window)?;
    assert!(owner.terminal_preset(registry, page, dir).await.is_err());
    owner.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn history_capacity_native_snapshot_save_reset_and_page_revocation() -> TestResult {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    let colored = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::TerminalPreset(pulse_store::settings::TerminalPreset::Paper),
        )
        .await?;
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            colored.token,
            Change::HistoryLimit(2500),
        )
        .await?;
    let startup = owner
        .terminal_startup(registry.clone(), page.clone(), dir.clone(), false)
        .await?;
    assert_eq!(startup.history_limit, 2500);
    assert_eq!(startup.preset, pulse_store::settings::TerminalPreset::Paper);
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            saved.token,
            Change::ResetHistoryLimit,
        )
        .await?;
    assert_eq!(reset.values.history_limit.value, 100_000);
    assert_eq!(
        reset.values.terminal_preset.value,
        pulse_store::settings::TerminalPreset::Paper
    );
    documents.started(window)?;
    assert!(
        owner
            .terminal_startup(registry, page, dir, false)
            .await
            .is_err()
    );
    owner.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn scroll_input_native_save_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert!(first.values.scroll_on_input.value);
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::ScrollOnInput(false),
        )
        .await?;
    assert!(!saved.values.scroll_on_input.value);
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert!(!reopened.values.scroll_on_input.value);
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            reopened.token,
            Change::ResetScrollOnInput,
        )
        .await?;
    assert!(reset.values.scroll_on_input.value);
    documents.started(window)?;
    assert!(owner.read(registry, page, dir).await.is_err());
    Ok(())
}

#[tokio::test]
async fn scroll_output_native_save_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert!(!first.values.scroll_on_output.value);
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::ScrollOnOutput(true),
        )
        .await?;
    assert!(saved.values.scroll_on_output.value);
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert!(reopened.values.scroll_on_output.value);
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            reopened.token,
            Change::ResetScrollOnOutput,
        )
        .await?;
    assert!(!reset.values.scroll_on_output.value);
    documents.started(window)?;
    assert!(owner.read(registry, page, dir).await.is_err());
    Ok(())
}

#[tokio::test]
async fn mac_option_native_save_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert!(!first.values.mac_option_as_meta.value);
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::MacOptionAsMeta(true),
        )
        .await?;
    assert!(saved.values.mac_option_as_meta.value);
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert!(reopened.values.mac_option_as_meta.value);
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            reopened.token,
            Change::ResetMacOptionAsMeta,
        )
        .await?;
    assert!(!reset.values.mac_option_as_meta.value);
    documents.started(window)?;
    assert!(owner.read(registry, page, dir).await.is_err());
    Ok(())
}

#[tokio::test]
async fn backspace_native_save_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert_eq!(first.values.backspace_send.value, "del");
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::BackspaceSend("bs".into()),
        )
        .await?;
    assert_eq!(saved.values.backspace_send.value, "bs");
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(reopened.values.backspace_send.value, "bs");
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            reopened.token,
            Change::ResetBackspaceSend,
        )
        .await?;
    assert_eq!(reset.values.backspace_send.value, "del");
    documents.started(window)?;
    assert!(owner.read(registry, page, dir).await.is_err());
    Ok(())
}

#[tokio::test]
async fn terminal_behavior_native_save_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use pulse_store::settings::TerminalType;
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    let saved = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::DeleteSend("del".into()),
        )
        .await?;
    assert_eq!(saved.values.delete_send.value, "del");
    let read = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            read.token,
            Change::TermType(TerminalType::Vt100),
        )
        .await?;
    let read = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(
        read.values.terminal_startup(false).term_type,
        TerminalType::Vt100
    );
    owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            read.token,
            Change::ResetDeleteSend,
        )
        .await?;
    let read = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(read.values.delete_send.value, "csi");
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            read.token,
            Change::ResetTermType,
        )
        .await?;
    assert_eq!(
        reset.values.terminal_startup(false).term_type,
        TerminalType::Xterm256color
    );
    documents.started(window)?;
    assert!(owner.read(registry, page, dir).await.is_err());
    Ok(())
}

#[tokio::test]
async fn ui_accent_persists_reopens_conflicts_and_rejects_retired_page() -> TestResult {
    use pulse_store::settings::UiAccent;
    let temporary = tempfile::tempdir()?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))?;
    let dir = temporary.path().join("settings");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert_eq!(first.values.ui_accent.value, UiAccent::Cyan);
    owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::UiAccent(UiAccent::Blue),
        )
        .await?;
    owner.shutdown().await?;
    let owner = Arc::new(SettingsOwner::new());
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(reopened.values.ui_accent.value, UiAccent::Blue);
    assert_eq!(reopened.values.ui_accent.source, SettingsSource::User);
    let external = "# retain external edit\n[ui]\nappearance='dark'\naccent='green'\n[terminal]\nfont_size=19\n";
    fs::write(dir.join("settings.toml"), external)?;
    assert!(
        owner
            .save(
                registry.clone(),
                page.clone(),
                dir.clone(),
                reopened.token,
                Change::UiAccent(UiAccent::Red)
            )
            .await
            .is_err()
    );
    assert_eq!(fs::read_to_string(dir.join("settings.toml"))?, external);
    let fresh = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            fresh.token,
            Change::ResetUiAccent,
        )
        .await?;
    assert_eq!(reset.values.ui_accent.value, UiAccent::Cyan);
    assert_eq!(reset.values.ui_accent.source, SettingsSource::Default);
    assert_eq!(reset.values.font_size.value, 19);
    assert!(fs::read_to_string(dir.join("settings.toml"))?.contains("# retain external edit"));
    documents.started(window)?;
    assert!(
        owner
            .save(
                registry.clone(),
                page.clone(),
                dir.clone(),
                reset.token,
                Change::UiAccent(UiAccent::Amber)
            )
            .await
            .is_err()
    );
    assert!(owner.read(registry, page, dir).await.is_err());
    owner.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn system_theme_policy_native_reopen_reset_and_page_revocation()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    use pulse_store::settings::{TerminalPreset, TerminalTheme, TerminalThemeMode};
    let temp = tempfile::tempdir()?;
    let dir = temp.path().join("preferences");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    let theme = TerminalTheme {
        mode: TerminalThemeMode::System,
        light: TerminalPreset::CatppuccinLatte,
        dark: TerminalPreset::Dracula,
    };
    owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::TerminalTheme(theme),
        )
        .await?;
    owner.shutdown().await?;
    let reopened = Arc::new(SettingsOwner::new());
    let read = reopened
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(read.values.terminal_theme.value, theme);
    assert_eq!(
        reopened
            .terminal_startup(registry.clone(), page.clone(), dir.clone(), true)
            .await?
            .preset,
        TerminalPreset::Dracula
    );
    assert_eq!(
        reopened
            .terminal_startup(registry.clone(), page.clone(), dir.clone(), false)
            .await?
            .preset,
        TerminalPreset::CatppuccinLatte
    );
    let reset = reopened
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            read.token,
            Change::ResetTerminalTheme,
        )
        .await?;
    assert_eq!(reset.values.terminal_theme.value, TerminalTheme::default());
    assert_eq!(
        reopened
            .terminal_startup(registry.clone(), page.clone(), dir.clone(), true)
            .await?
            .preset,
        TerminalPreset::Classic
    );
    documents.started(window)?;
    assert!(
        reopened
            .save(
                registry,
                page,
                dir,
                reset.token,
                Change::TerminalTheme(theme)
            )
            .await
            .is_err()
    );
    reopened.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn ui_language_persists_reopens_conflicts_and_rejects_retired_page() -> TestResult {
    use pulse_store::settings::UiLanguage;
    let temporary = tempfile::tempdir()?;
    fs::set_permissions(temporary.path(), fs::Permissions::from_mode(0o700))?;
    let dir = temporary.path().join("settings");
    let (registry, documents, window, page) = page()?;
    let owner = Arc::new(SettingsOwner::new());
    let first = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(first.version, 15);
    assert_eq!(first.values.ui_language.value, UiLanguage::Chinese);
    owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            first.token,
            Change::UiLanguage(UiLanguage::English),
        )
        .await?;
    owner.shutdown().await?;
    let owner = Arc::new(SettingsOwner::new());
    let reopened = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    assert_eq!(reopened.values.ui_language.value, UiLanguage::English);
    assert_eq!(reopened.values.ui_language.source, SettingsSource::User);
    let external = "# retain external edit\n[ui]\nappearance='dark'\naccent='green'\nlanguage='en'\n[terminal]\nfont_size=19\n";
    fs::write(dir.join("settings.toml"), external)?;
    assert!(
        owner
            .save(
                registry.clone(),
                page.clone(),
                dir.clone(),
                reopened.token,
                Change::UiLanguage(UiLanguage::Chinese)
            )
            .await
            .is_err()
    );
    assert_eq!(fs::read_to_string(dir.join("settings.toml"))?, external);
    let fresh = owner
        .read(registry.clone(), page.clone(), dir.clone())
        .await?;
    let reset = owner
        .save(
            registry.clone(),
            page.clone(),
            dir.clone(),
            fresh.token,
            Change::ResetUiLanguage,
        )
        .await?;
    assert_eq!(reset.values.ui_language.value, UiLanguage::Chinese);
    assert_eq!(reset.values.ui_language.source, SettingsSource::Default);
    assert_eq!(reset.values.font_size.value, 19);
    assert!(fs::read_to_string(dir.join("settings.toml"))?.contains("# retain external edit"));
    documents.started(window)?;
    assert!(
        owner
            .save(
                registry.clone(),
                page.clone(),
                dir.clone(),
                reset.token,
                Change::UiLanguage(UiLanguage::English)
            )
            .await
            .is_err()
    );
    assert!(owner.read(registry, page, dir).await.is_err());
    owner.shutdown().await?;
    Ok(())
}
