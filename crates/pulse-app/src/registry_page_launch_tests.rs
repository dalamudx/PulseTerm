//! Rust backend generation checks; this fixture does not authenticate a Tauri page.
#![allow(clippy::unwrap_used)]
use super::*;

#[test]
fn rotating_or_retiring_page_revokes_only_its_tickets_and_retains_quota() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let first = registry.activate_launch_page(owner).unwrap();
    let old = registry.begin_page_local_launch(&first).unwrap();
    let second = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        old.require_current(),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry.begin_page_local_launch(&first),
        Err(RegistryError::Forbidden)
    ));
    let newest = registry.begin_page_local_launch(&second).unwrap();
    drop(first); // A stale scope cannot revoke its replacement.
    newest.require_current().unwrap();
    let others = (0..62)
        .map(|_| registry.begin_page_local_launch(&second).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_page_local_launch(&second),
        Err(RegistryError::Busy)
    ));
    assert_eq!(registry.retire_window_views(owner).unwrap(), 0);
    assert!(matches!(
        newest.require_current(),
        Err(RegistryError::Forbidden)
    ));
    let third = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        registry.begin_page_local_launch(&third),
        Err(RegistryError::Busy)
    ));
    drop(old);
    let current = registry.begin_page_local_launch(&third).unwrap();
    current.require_current().unwrap();
    drop(others);
    drop(newest);
    drop(current);
    drop(third);
    assert!(registry.begin_local_launch(owner).is_ok());
}

#[test]
fn page_scope_is_registry_bound_and_exhaustion_does_not_invalidate_current() {
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        foreign.begin_page_local_launch(&page),
        Err(RegistryError::Forbidden)
    ));
    registry.lock().unwrap().next_page = u64::MAX;
    assert!(matches!(
        registry.activate_launch_page(owner),
        Err(RegistryError::Unavailable)
    ));
    registry
        .begin_page_local_launch(&page)
        .unwrap()
        .require_current()
        .unwrap();
    registry.close_window(owner).unwrap();
    assert!(matches!(
        registry.begin_page_local_launch(&page),
        Err(RegistryError::Forbidden)
    ));
}

#[test]
fn read_only_page_check_is_current_and_never_uses_launch_credit() {
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let old = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        foreign.check_launch_page(&old),
        Err(RegistryError::Forbidden)
    ));
    assert!(registry.check_launch_page(&old).is_ok());
    let tickets = (0..64)
        .map(|_| registry.begin_page_local_launch(&old).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_page_local_launch(&old),
        Err(RegistryError::Busy)
    ));
    assert!(registry.check_launch_page(&old).is_ok());
    let fresh = registry.activate_launch_page(owner).unwrap();
    assert!(matches!(
        registry.check_launch_page(&old),
        Err(RegistryError::Forbidden)
    ));
    assert!(registry.check_launch_page(&fresh).is_ok());
    registry.close_window(owner).unwrap();
    assert!(matches!(
        registry.check_launch_page(&fresh),
        Err(RegistryError::Forbidden)
    ));
    drop(tickets);
}

#[tokio::test]
async fn page_rotation_and_navigation_do_not_close_a_committed_actor() {
    use pulse_term::session::Lifecycle;
    use tokio::time::{Duration, timeout};
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let page = registry.activate_launch_page(owner).unwrap();
    let mut ticket = registry.begin_page_local_launch(&page).unwrap();
    let (handle, task, driver) = super::tests::session();
    let id = registry
        .commit_local_launch(&mut ticket, handle.clone())
        .unwrap();
    drop(ticket);
    let next = registry.activate_launch_page(owner).unwrap();
    drop(page);
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    assert_eq!(registry.retire_window_views(owner).unwrap(), 0);
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    assert!(matches!(registry.reap_closed(id), Err(RegistryError::Busy)));
    drop(next);
    handle.close();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
    registry.reap_closed(id).unwrap();
}
