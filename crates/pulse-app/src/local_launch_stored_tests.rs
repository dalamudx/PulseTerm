//! Private SQLite preferences → trusted discovery → current page → real PTY.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_pty::profiles::{ProfileError, ShellCandidate, ShellKind};
use pulse_store::profiles::{AutoProfileEdit, AutomaticShell};
use pulse_term::session::Lifecycle;
use std::os::unix::fs::PermissionsExt;
use tokio::time::{Duration, timeout};

#[tokio::test(flavor = "current_thread")]
async fn stored_tombstone_and_failed_store_never_fall_back_to_launch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    let size = TermSize::new(32, 8).unwrap();
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/bin/true".into()).unwrap()];
    let spec = StoredProfileLaunch {
        candidates: &candidates,
        kind: ShellKind::Sh,
        context: &context,
        size,
        history: 16,
    };
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await
        .unwrap();
    assert!(matches!(
        start_page_stored_default_profile_local(&registry, &page, &store, spec).await,
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::NotFound
        )))
    ));
    store
        .edit_auto_profiles(AutoProfileEdit::RestoreDeleted(AutomaticShell::Sh))
        .await
        .unwrap();
    let pending = start_page_stored_default_profile_local(&registry, &page, &store, spec)
        .await
        .unwrap();
    let ready = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap()
        .unwrap();
    let receipt = timeout(Duration::from_secs(5), ready.completion)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(receipt.actor.state.lifecycle, Lifecycle::Ended);
    assert_eq!(receipt.native.exit_code, 0);
    registry.reap_closed(ready.id).unwrap();
    registry.retire_window_views(owner).unwrap();
    assert!(matches!(
        start_page_stored_default_profile_local(&registry, &page, &store, spec).await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    drop(store);
    task.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn closed_database_worker_is_not_an_empty_catalog() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    task.shutdown().await.unwrap();
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/bin/true".into()).unwrap()];
    let spec = StoredProfileLaunch {
        candidates: &candidates,
        kind: ShellKind::Sh,
        context: &context,
        size: TermSize::new(32, 8).unwrap(),
        history: 16,
    };
    assert!(matches!(
        start_page_stored_default_profile_local(&registry, &page, &store, spec).await,
        Err(LocalStartError::Store(StoreError::Closed))
    ));
}

#[tokio::test(flavor = "current_thread")]
async fn profile_offers_only_project_current_identities_without_paths() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let old = registry.activate_launch_page(owner).unwrap();
    let candidates = [
        ShellCandidate::new(ShellKind::Bash, "/bin/bash".into()).unwrap(),
        ShellCandidate::new(ShellKind::Sh, "/bin/sh".into()).unwrap(),
    ];
    store
        .edit_auto_profiles(AutoProfileEdit::Visibility {
            shell: AutomaticShell::Bash,
            hidden: true,
        })
        .await
        .unwrap();
    store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await
        .unwrap();
    assert!(matches!(
        list_page_profile_offers(&foreign, &old, &store, &candidates).await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    assert_eq!(
        list_page_profile_offers(&registry, &old, &store, &candidates)
            .await
            .unwrap(),
        vec![ProfileOffer {
            kind: ShellKind::Bash,
            hidden: true
        }]
    );
    store
        .edit_auto_profiles(AutoProfileEdit::RestoreDeleted(AutomaticShell::Sh))
        .await
        .unwrap();
    let offers = list_page_profile_offers(&registry, &old, &store, &candidates)
        .await
        .unwrap();
    assert_eq!(offers.len(), 2);
    assert!(offers.contains(&ProfileOffer {
        kind: ShellKind::Bash,
        hidden: true
    }));
    assert!(offers.contains(&ProfileOffer {
        kind: ShellKind::Sh,
        hidden: false
    }));
    let current = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        list_page_profile_offers(&registry, &old, &store, &candidates).await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    assert_eq!(
        list_page_profile_offers(&registry, &current, &store, &candidates)
            .await
            .unwrap(),
        offers
    );
    drop(store);
    task.shutdown().await.unwrap();
}

#[tokio::test(flavor = "current_thread")]
async fn stored_profile_palette_override_inheritance_and_reset_reach_real_osc_query() {
    use pulse_store::{
        profile_editor::ProfileConfig,
        settings::{SettingsResolver, TerminalPreset},
    };
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/bin/sh".into()).unwrap()];
    let spec = StoredProfileLaunch {
        candidates: &candidates,
        kind: ShellKind::Sh,
        context: &context,
        size: TermSize::new(32, 8).unwrap(),
        history: 16,
    };
    let global = SettingsResolver::resolve(
        Some(
            r#"[terminal]
theme={mode="system",light="catppuccin_latte",dark="dracula"}
"#,
        ),
        None,
        None,
    )
    .unwrap();
    for (override_preset, system_dark, expected) in [
        (
            Some(TerminalPreset::CatppuccinMocha),
            true,
            TerminalPreset::CatppuccinMocha,
        ),
        (None, true, TerminalPreset::Dracula),
        (None, false, TerminalPreset::CatppuccinLatte),
    ] {
        let [r, g, b] = expected.colors().background;
        let reply = format!(
            "\x1b]11;rgb:{:04x}/{:04x}/{:04x}\x07",
            u16::from(r) * 257,
            u16::from(g) * 257,
            u16::from(b) * 257
        );
        let mut config = ProfileConfig::default();
        config.appearance.color_preset = override_preset;
        config.args = vec![
            "-c".into(),
            format!(
                "stty -echo -icanon min 1 time 20; printf '\\033]11;?\\007'; result=$(dd bs=1 count={} 2>/dev/null); test \"$result\" = '{}'",
                reply.len(),
                reply
            ),
        ];
        let before = store.load_profile_editor(AutomaticShell::Sh).await.unwrap();
        store
            .save_profile_editor(before, config, || true)
            .await
            .unwrap();
        let (pending, appearance) = start_page_stored_configured_local_with_preset(
            &registry,
            &page,
            &store,
            spec,
            global.terminal_startup(system_dark),
        )
        .await
        .unwrap();
        assert_eq!(appearance.color_preset, override_preset);
        let ready = timeout(Duration::from_secs(5), pending.wait())
            .await
            .unwrap()
            .unwrap();
        let receipt = timeout(Duration::from_secs(5), ready.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(
            receipt.native.exit_code, 0,
            "OSC 11 must answer using the frozen effective palette"
        );
        assert_eq!(receipt.actor.state.lifecycle, Lifecycle::Ended);
        registry.reap_closed(ready.id).unwrap();
    }
    registry.retire_window_views(owner).unwrap();
    drop(store);
    task.shutdown().await.unwrap();
}
