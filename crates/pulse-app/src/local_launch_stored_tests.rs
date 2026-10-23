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
    let dir = crate::test_tempdir().unwrap();
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
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/usr/bin/true".into()).unwrap()];
    let spec = StoredProfileLaunch {
        cwd: None,
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
    let dir = crate::test_tempdir().unwrap();
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
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/usr/bin/true".into()).unwrap()];
    let spec = StoredProfileLaunch {
        cwd: None,
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
    let dir = crate::test_tempdir().unwrap();
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
    let dir = crate::test_tempdir().unwrap();
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
        cwd: None,
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

#[tokio::test]
async fn one_shot_directory_overrides_automatic_and_custom_profiles_without_persistence_or_fallback()
 {
    use pulse_store::{
        custom_profiles::{CustomProfileDraft, CustomProfileEdit},
        profile_editor::ProfileConfig,
    };
    let dir = crate::test_tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let work = dir.path().join("中文 workspace");
    std::fs::create_dir(&work).unwrap();
    let cwd = LaunchDirectory::parse(work.to_str().unwrap()).unwrap();
    let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
    let config = ProfileConfig {
        cwd: Some(dir.path().to_str().unwrap().into()),
        args: vec!["-c".into(), "test \"$PWD\" = \"$EXPECTED_CWD\"".into()],
        env: vec![("EXPECTED_CWD".into(), cwd.as_str().into())],
        ..ProfileConfig::default()
    };
    let before = store.load_profile_editor(AutomaticShell::Sh).await.unwrap();
    store
        .save_profile_editor(before, config.clone(), || true)
        .await
        .unwrap();
    let before = store.load_custom_profiles().await.unwrap();
    let created = store
        .edit_custom_profiles(
            before,
            CustomProfileEdit::Create(CustomProfileDraft {
                name: "cwd override".into(),
                executable: "/bin/sh".into(),
                hidden: false,
                config: config.clone(),
            }),
            || true,
        )
        .await
        .unwrap();
    let id = created.profiles()[0].id();
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "cwd-test".into(),
        pane: None,
    };
    let candidates = [ShellCandidate::new(ShellKind::Sh, "/bin/sh".into()).unwrap()];
    let spec = StoredProfileLaunch {
        cwd: Some(&cwd),
        candidates: &candidates,
        kind: ShellKind::Sh,
        context: &context,
        size: TermSize::new(32, 8).unwrap(),
        history: 16,
    };
    for custom in [false, true] {
        let pending = if custom {
            start_page_custom_local_with_preset(
                &registry,
                &page,
                &store,
                CustomProfileLaunch {
                    cwd: Some(&cwd),
                    id,
                    context: &context,
                    size: spec.size,
                    store_retention: Box::new(()),
                },
                pulse_store::settings::TerminalPreset::Classic.into(),
            )
            .await
            .unwrap()
            .0
        } else {
            start_page_stored_default_profile_local(&registry, &page, &store, spec)
                .await
                .unwrap()
        };
        let ready = timeout(Duration::from_secs(5), pending.wait())
            .await
            .unwrap()
            .unwrap();
        let receipt = timeout(Duration::from_secs(5), ready.completion)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(receipt.native.exit_code, 0);
        registry.reap_closed(ready.id).unwrap();
    }
    assert_eq!(
        store
            .load_profile_editor(AutomaticShell::Sh)
            .await
            .unwrap()
            .config(),
        config
    );
    assert_eq!(
        store
            .load_custom_profiles()
            .await
            .unwrap()
            .active(id)
            .unwrap()
            .draft()
            .config,
        config
    );
    std::fs::remove_dir(work).unwrap();
    if let Ok(pending) =
        start_page_stored_default_profile_local(&registry, &page, &store, spec).await
    {
        assert!(
            timeout(Duration::from_secs(5), pending.wait())
                .await
                .unwrap()
                .is_err()
        );
    }
    let _new_page = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        start_page_stored_default_profile_local(&registry, &page, &store, spec).await,
        Err(LocalStartError::Registry(_))
    ));
    drop(store);
    task.shutdown().await.unwrap();
}
