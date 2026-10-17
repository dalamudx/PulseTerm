//! Backend page-generation fixture, not a real Tauri PageLoad/nonce authorization test.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_pty::{
    launch::{LocalSessionEnvironment, LocalShellMode},
    profiles::{AutoProfileCatalog, AutoProfileLaunch, ShellCandidate, ShellKind},
};
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

#[tokio::test(flavor = "current_thread")]
async fn retired_page_cannot_spawn_and_new_page_still_opens_the_same_profile() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let mut catalog = AutoProfileCatalog::default();
    catalog
        .reconcile(&[ShellCandidate::new(ShellKind::Bash, "/bin/bash".into()).unwrap()])
        .unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    let size = TermSize::new(32, 8).unwrap();
    let choose = || ProfileSelection {
        kind: ShellKind::Bash,
        options: AutoProfileLaunch {
            args: ["--noprofile", "--norc", "-c", "printf 'PAGE_OK\\n'"]
                .map(Into::into)
                .into(),
            mode: LocalShellMode::Plain,
            ..AutoProfileLaunch::default()
        },
    };
    let old = registry.activate_launch_page(owner).unwrap();
    let pending =
        start_page_profile_local(&registry, &old, &catalog, choose(), &context, size, 16).unwrap();
    let current = registry.activate_launch_page(owner).unwrap();
    let outcome = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap();
    assert!(matches!(
        outcome,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    assert!(matches!(
        start_page_profile_local(&registry, &old, &catalog, choose(), &context, size, 16),
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    drop(old);
    let pending =
        start_page_profile_local(&registry, &current, &catalog, choose(), &context, size, 16)
            .unwrap();
    let ready = timeout(Duration::from_secs(5), pending.wait())
        .await
        .unwrap()
        .unwrap();
    let result = timeout(Duration::from_secs(5), ready.completion)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(result.actor.state.lifecycle, Lifecycle::Ended);
    assert!(result.actor.state.parsed_bytes >= "PAGE_OK\r\n".len() as u64);
    assert_eq!(result.native.exit_code, 0);
    assert_eq!(registry.retire_window_views(owner).unwrap(), 0);
    assert!(matches!(
        start_page_profile_local(&registry, &current, &catalog, choose(), &context, size, 16),
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    registry.reap_closed(ready.id).unwrap();
}
