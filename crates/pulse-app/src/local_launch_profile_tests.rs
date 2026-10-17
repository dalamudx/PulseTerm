// Intentionally separate from arbitrary-command fixture: the executable must come from the catalog.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_pty::{
    launch::{LocalSessionEnvironment, LocalShellMode},
    profiles::{AutoProfileCatalog, AutoProfileLaunch, ProfileError, ShellCandidate, ShellKind},
};
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn current_profile_launch_and_missing_unavailable_deleted_reject_without_fallback() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let mut catalog = AutoProfileCatalog::default();
    let bash = ShellCandidate::new(ShellKind::Bash, "/bin/bash".into()).unwrap();
    catalog.reconcile(&[bash]).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    let rejected = start_profile_local(
        &registry,
        owner,
        &catalog,
        ProfileSelection {
            kind: ShellKind::Zsh,
            options: AutoProfileLaunch::default(),
        },
        &context,
        TermSize::new(32, 8).unwrap(),
        16,
    );
    assert!(matches!(
        rejected,
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::NotFound
        )))
    ));

    let options = AutoProfileLaunch {
        args: ["--noprofile", "--norc", "-c", "printf 'PROFILE_OK\\n'"]
            .map(Into::into)
            .into(),
        mode: LocalShellMode::Plain,
        ..AutoProfileLaunch::default()
    };
    let pending = start_profile_local(
        &registry,
        owner,
        &catalog,
        ProfileSelection {
            kind: ShellKind::Bash,
            options,
        },
        &context,
        TermSize::new(32, 8).unwrap(),
        16,
    )
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
    assert!(result.actor.state.parsed_bytes >= "PROFILE_OK\r\n".len() as u64);
    assert_eq!(result.native.exit_code, 0);
    registry.reap_closed(ready.id).unwrap();

    catalog.reconcile(&[]).unwrap();
    let rejected = start_profile_local(
        &registry,
        owner,
        &catalog,
        ProfileSelection {
            kind: ShellKind::Bash,
            options: AutoProfileLaunch::default(),
        },
        &context,
        TermSize::new(32, 8).unwrap(),
        16,
    );
    assert!(matches!(
        rejected,
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::Unavailable
        )))
    ));
    catalog.remove(ShellKind::Bash).unwrap();
    catalog
        .reconcile(&[ShellCandidate::new(ShellKind::Bash, "/bin/bash".into()).unwrap()])
        .unwrap();
    let rejected = start_profile_local(
        &registry,
        owner,
        &catalog,
        ProfileSelection {
            kind: ShellKind::Bash,
            options: AutoProfileLaunch::default(),
        },
        &context,
        TermSize::new(32, 8).unwrap(),
        16,
    );
    assert!(matches!(
        rejected,
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::NotFound
        )))
    ));
    let still_available = registry.begin_local_launch(owner).unwrap();
    drop(still_available);
}
