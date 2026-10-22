//! A synthetic catalog proves identity-only admission without reading real shell startup files.
#![cfg(unix)]
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_pty::{
    launch::LocalSessionEnvironment,
    profiles::{AutoProfileCatalog, ProfileError, ShellCandidate, ShellKind},
};
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

#[tokio::test(flavor = "current_thread")]
async fn only_selected_current_catalog_identity_can_launch_with_default_options() {
    assert!(std::path::Path::new("/usr/bin/true").is_file());
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let context = LocalSessionEnvironment {
        application_version: "test-version".into(),
        pane: None,
    };
    let size = TermSize::new(32, 8).unwrap();
    let mut catalog = AutoProfileCatalog::default();
    // Only the isolated fixture can claim a non-shell executable as Sh. Real callers
    // must obtain ShellCandidates from the backend discovery, never from page input.
    catalog
        .reconcile(&[
            ShellCandidate::new(ShellKind::Sh, "/usr/bin/true".into()).unwrap(),
            ShellCandidate::new(ShellKind::Bash, "/usr/bin/false".into()).unwrap(),
        ])
        .unwrap();
    assert!(matches!(
        start_page_default_profile_local(
            &registry,
            &page,
            &catalog,
            ShellKind::Fish,
            &context,
            size,
            16,
        ),
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::NotFound
        )))
    ));
    let pending = start_page_default_profile_local(
        &registry,
        &page,
        &catalog,
        ShellKind::Sh,
        &context,
        size,
        16,
    )
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
    assert_eq!(receipt.native.exit_code, 0);
    assert_eq!(receipt.actor.state.lifecycle, Lifecycle::Ended);
    registry.reap_closed(ready.id).unwrap();
    catalog.remove(ShellKind::Sh).unwrap();
    assert!(matches!(
        start_page_default_profile_local(
            &registry,
            &page,
            &catalog,
            ShellKind::Sh,
            &context,
            size,
            16,
        ),
        Err(LocalStartError::Profile(ProfileLaunchError::Profile(
            ProfileError::NotFound
        )))
    ));
    registry.retire_window_views(owner).unwrap();
    assert!(matches!(
        start_page_default_profile_local(
            &registry,
            &page,
            &catalog,
            ShellKind::Bash,
            &context,
            size,
            16,
        ),
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
}
