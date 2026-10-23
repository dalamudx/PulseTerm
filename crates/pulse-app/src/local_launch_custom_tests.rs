//! Saved UUID → literal program/config → original-page real PTY. No shell discovery fallback.
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_store::custom_profiles::{CustomProfileDraft, CustomProfileEdit};
use pulse_store::profile_editor::{ProfileAppearance, ProfileConfig};
use pulse_store::settings::{SettingsResolver, TerminalPreset};
use std::os::unix::fs::PermissionsExt;
use tokio::time::{Duration, timeout};
#[tokio::test]
async fn saved_custom_profile_runs_literals_with_frozen_colors_and_rejects_deleted_or_old_pages() {
    let dir = crate::test_tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let db = dir.path().join("state.db");
    let (store, task) = StateStore::open(db.clone()).await.unwrap();
    let config = ProfileConfig {
        args: vec!["-c".into(), "test \"$#\" = 2 && test -z \"$1\" && test \"$2\" = ' ; $(literal) 中文🌍 ' && test \"$PROFILE_VALUE\" = 'literal = value' && test \"$PWD\" = \"$EXPECTED_CWD\" && test \"$TERM\" = xterm-256color && test \"$TERM_PROGRAM\" = PulseTerm; passed=$?; stty -echo -icanon min 1 time 20; printf '\\033]11;?\\007'; reply=$(dd bs=1 count=24 2>/dev/null); test \"$passed\" = 0 && test \"$reply\" = \"$(printf '\\033]11;rgb:1e1e/1e1e/2e2e\\007')\"".into(), "profile-literal".into(), "".into(), " ; $(literal) 中文🌍 ".into()],
        cwd: Some(dir.path().to_str().unwrap().to_owned()),
        env: vec![
            ("PROFILE_VALUE".into(), "literal = value".into()),
            ("EXPECTED_CWD".into(), dir.path().to_str().unwrap().to_owned()),
        ],
        appearance: ProfileAppearance { color_preset: Some(TerminalPreset::CatppuccinMocha), ..ProfileAppearance::default() },
        ..ProfileConfig::default()
    };
    let snapshot = store.load_custom_profiles().await.unwrap();
    let created = store
        .edit_custom_profiles(
            snapshot,
            CustomProfileEdit::Create(CustomProfileDraft {
                name: "literal profile".into(),
                executable: "/bin/sh".into(),
                hidden: false,
                config,
            }),
            || true,
        )
        .await
        .unwrap();
    let id = created.profiles()[0].id();
    drop(store);
    task.shutdown().await.unwrap();
    let (store, task) = StateStore::open(db).await.unwrap();
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "custom-test".into(),
        pane: None,
    };
    let spec = || CustomProfileLaunch {
        cwd: None,
        id,
        context: &context,
        size: TermSize::new(80, 24).unwrap(),
        store_retention: Box::new(()),
    };
    let startup = SettingsResolver::resolve(None, None, None)
        .unwrap()
        .terminal_startup(false);
    let (pending, appearance) =
        start_page_custom_local_with_preset(&registry, &page, &store, spec(), startup)
            .await
            .unwrap();
    assert_eq!(
        appearance.color_preset,
        Some(TerminalPreset::CatppuccinMocha)
    );
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
        "literal args, env, cwd and OSC 11 must use saved config"
    );
    registry.reap_closed(ready.id).unwrap();
    let before = store.load_custom_profiles().await.unwrap();
    let deleted = store
        .edit_custom_profiles(before, CustomProfileEdit::Delete(id), || true)
        .await
        .unwrap();
    assert!(
        start_page_custom_local_with_preset(&registry, &page, &store, spec(), startup)
            .await
            .is_err()
    );
    store
        .edit_custom_profiles(deleted, CustomProfileEdit::Restore(id), || true)
        .await
        .unwrap();
    registry.retire_window_views(owner).unwrap();
    assert!(matches!(
        start_page_custom_local_with_preset(&registry, &page, &store, spec(), startup).await,
        Err(LocalStartError::Registry(_))
    ));
    drop(store);
    task.shutdown().await.unwrap();
}
