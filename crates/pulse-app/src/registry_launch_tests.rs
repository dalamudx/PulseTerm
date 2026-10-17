//! Internal pre-spawn quota/ownership tests. A reservation does not authorize a page or shell.
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn reservations_prevent_unbounded_spawn_and_release_without_affecting_sessions() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let mut tickets = (0..64)
        .map(|_| registry.begin_local_launch(owner).unwrap())
        .collect::<Vec<_>>();
    assert!(matches!(
        registry.begin_local_launch(owner),
        Err(RegistryError::Busy)
    ));
    let (extra, extra_task, extra_driver) = super::tests::session();
    assert!(matches!(
        registry.register(owner, extra.clone()),
        Err(RegistryError::Busy)
    ));
    extra.close();
    drop(extra_driver);
    timeout(Duration::from_secs(2), extra_task.join())
        .await
        .unwrap()
        .unwrap();
    drop(tickets.pop());
    let (handle, task, driver) = super::tests::session();
    let id = registry.register(owner, handle.clone()).unwrap();
    assert!(matches!(
        registry.begin_local_launch(owner),
        Err(RegistryError::Busy)
    ));
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    drop(tickets);
    let unused = registry.begin_local_launch(owner).unwrap();
    drop(unused);
    assert_eq!(registry.lock().unwrap().launches.len(), 0);
    registry.close_session(owner, id).unwrap();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn commit_checks_registry_and_live_window_without_killing_existing_session() {
    let registry = SessionRegistry::new();
    let foreign = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let (existing, existing_task, existing_driver) = super::tests::session();
    let id = registry.register(owner, existing.clone()).unwrap();
    let mut ticket = registry.begin_local_launch(owner).unwrap();
    let (rejected, rejected_task, rejected_driver) = super::tests::session();
    assert!(matches!(
        foreign.commit_local_launch(&mut ticket, rejected.clone()),
        Err(RegistryError::Forbidden)
    ));
    rejected.close();
    drop(rejected_driver);
    timeout(Duration::from_secs(2), rejected_task.join())
        .await
        .unwrap()
        .unwrap();
    drop(ticket);
    assert!(registry.lock().unwrap().launches.is_empty());
    let mut stale = registry.begin_local_launch(owner).unwrap();
    registry.close_window(owner).unwrap();
    let (late, late_task, late_driver) = super::tests::session();
    assert!(matches!(
        registry.commit_local_launch(&mut stale, late.clone()),
        Err(RegistryError::Forbidden)
    ));
    late.close();
    drop(late_driver);
    timeout(Duration::from_secs(2), late_task.join())
        .await
        .unwrap()
        .unwrap();
    drop(stale);
    assert!(matches!(
        registry.begin_local_launch(owner),
        Err(RegistryError::Forbidden)
    ));
    assert_eq!(existing.state.borrow().lifecycle, Lifecycle::Running);
    let mut ticket = registry.begin_local_launch(other).unwrap();
    let (replacement, replacement_task, replacement_driver) = super::tests::session();
    let new_id = registry
        .commit_local_launch(&mut ticket, replacement.clone())
        .unwrap();
    assert_eq!(replacement.state.borrow().lifecycle, Lifecycle::Running);
    registry.close_session(other, new_id).unwrap();
    drop(replacement_driver);
    timeout(Duration::from_secs(2), replacement_task.join())
        .await
        .unwrap()
        .unwrap();
    existing.close();
    drop(existing_driver);
    timeout(Duration::from_secs(2), existing_task.join())
        .await
        .unwrap()
        .unwrap();
    registry.reap_closed(id).unwrap();
}
#[tokio::test]
async fn exhaustion_and_shutdown_do_not_admit_a_stale_launch() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    registry.lock().unwrap().next_launch = u64::MAX;
    assert!(matches!(
        registry.begin_local_launch(owner),
        Err(RegistryError::Unavailable)
    ));
    assert!(registry.lock().unwrap().launches.is_empty());
    registry.lock().unwrap().next_launch = 1;
    let mut ticket = registry.begin_local_launch(owner).unwrap();
    registry.shutdown().unwrap();
    let (late, task, driver) = super::tests::session();
    assert!(matches!(
        registry.commit_local_launch(&mut ticket, late.clone()),
        Err(RegistryError::Forbidden)
    ));
    late.close();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
    drop(ticket);
    assert!(registry.lock().unwrap().launches.is_empty());
}

#[tokio::test]
async fn duplicate_actor_and_session_id_exhaustion_release_ticket_not_existing_actor() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (existing, task, driver) = super::tests::session();
    let id = registry.register(owner, existing.clone()).unwrap();
    let mut duplicate = registry.begin_local_launch(owner).unwrap();
    assert!(matches!(
        registry.commit_local_launch(&mut duplicate, existing.clone()),
        Err(RegistryError::Forbidden)
    ));
    drop(duplicate); // This was an already-registered actor, not a new child.
    assert!(registry.lock().unwrap().launches.is_empty());
    assert_eq!(existing.state.borrow().lifecycle, Lifecycle::Running);
    registry.lock().unwrap().next_session = u64::MAX;
    let mut exhausted = registry.begin_local_launch(owner).unwrap();
    let (late, late_task, late_driver) = super::tests::session();
    assert!(matches!(
        registry.commit_local_launch(&mut exhausted, late.clone()),
        Err(RegistryError::Unavailable)
    ));
    assert_eq!(registry.lock().unwrap().launches.len(), 1);
    late.close();
    drop(late_driver);
    timeout(Duration::from_secs(2), late_task.join())
        .await
        .unwrap()
        .unwrap();
    drop(exhausted);
    assert!(registry.lock().unwrap().launches.is_empty());
    assert_eq!(existing.state.borrow().lifecycle, Lifecycle::Running);
    registry.close_session(owner, id).unwrap();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn real_local_child_reclaims_failed_launch_and_survives_window_close_after_commit() {
    use crate::spawn_local;
    use pulse_pty::PtyCommand;
    // Fixed test-only program; no frontend can pass a path or argv to this code.
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let mut ticket = registry.begin_local_launch(owner).unwrap();
    let mut cmd = PtyCommand::new("/bin/sh");
    cmd.args = vec!["-c".into(), "read x".into()];
    let (uncommitted, task) = spawn_local(cmd, TermSize::new(32, 8).unwrap(), 16)
        .await
        .unwrap();
    registry.close_window(owner).unwrap();
    assert!(matches!(
        registry.commit_local_launch(&mut ticket, uncommitted.clone()),
        Err(RegistryError::Forbidden)
    ));
    uncommitted.close();
    timeout(Duration::from_secs(5), task.join())
        .await
        .unwrap()
        .unwrap();
    drop(ticket);
    assert!(registry.lock().unwrap().launches.is_empty());
    let next = registry.open_window().unwrap();
    let mut ticket = registry.begin_local_launch(next).unwrap();
    let mut cmd = PtyCommand::new("/bin/sh");
    cmd.args = vec![
        "-c".into(),
        "printf 'READY\\n'; sleep 1; printf 'DONE\\n'".into(),
    ];
    let (handle, task) = spawn_local(cmd, TermSize::new(32, 8).unwrap(), 16)
        .await
        .unwrap();
    let id = registry
        .commit_local_launch(&mut ticket, handle.clone())
        .unwrap();
    registry.close_window(next).unwrap();
    let (state, native) = timeout(Duration::from_secs(5), task.join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(state.lifecycle, Lifecycle::Ended);
    assert!(state.parsed_bytes >= "READY\r\nDONE\r\n".len() as u64);
    assert_eq!(native.exit_code, 0);
    registry.reap_closed(id).unwrap();
}

#[tokio::test]
async fn revoked_tickets_hold_quota_until_supervisor_drops_them() {
    let registry = SessionRegistry::new();
    let source = registry.open_window().unwrap();
    let mut pending = (0..64)
        .map(|_| registry.begin_local_launch(source).unwrap())
        .collect::<Vec<_>>();
    registry.close_window(source).unwrap();
    let target = registry.open_window().unwrap();
    assert_eq!(registry.lock().unwrap().launches.len(), 64);
    assert!(
        registry
            .lock()
            .unwrap()
            .launches
            .values()
            .all(|item| item.revoked)
    );
    assert!(matches!(
        registry.begin_local_launch(target),
        Err(RegistryError::Busy)
    ));
    let (rejected, task, driver) = super::tests::session();
    let mut canceled = pending.pop().unwrap();
    assert!(matches!(
        registry.commit_local_launch(&mut canceled, rejected.clone()),
        Err(RegistryError::Forbidden)
    ));
    rejected.close();
    drop(driver);
    timeout(Duration::from_secs(2), task.join())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(registry.lock().unwrap().launches.len(), 64);
    drop(canceled);
    assert_eq!(registry.lock().unwrap().launches.len(), 63);
    let fresh = registry.begin_local_launch(target).unwrap();
    assert!(matches!(
        registry.begin_local_launch(target),
        Err(RegistryError::Busy)
    ));
    drop(fresh);
    drop(pending);
    assert!(registry.lock().unwrap().launches.is_empty());
}
