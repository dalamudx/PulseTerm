//! Actual Registry/actor ownership checks; these do not substitute for native multiwindow validation.
#![allow(clippy::unwrap_used)]
use super::tests::session;
use super::*;
use pulse_term::session::Lifecycle;
use tokio::time::{Duration, timeout};

#[tokio::test]
async fn root_listing_hides_foreign_sessions_and_does_not_grant_close_or_attach() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let empty = r.open_window().unwrap();
    let pa = r.activate_launch_page(a).unwrap();
    let pb = r.activate_launch_page(b).unwrap();
    let pe = r.activate_launch_page(empty).unwrap();
    let (ha, ta, da) = session();
    let (hb, tb, db) = session();
    let ia = r.register(a, ha).unwrap();
    let ib = r.register(b, hb).unwrap();
    let all = [ib, ia];
    assert_eq!(r.owned_page_sessions(&pa, &all).unwrap(), vec![ia]);
    assert_eq!(r.owned_page_sessions(&pb, &all).unwrap(), vec![ib]);
    assert!(r.owned_page_sessions(&pe, &all).unwrap().is_empty());
    assert!(r.resolve(a, ib.get()).is_err());
    assert!(r.request_close_page_session(&pa, ib).is_err());
    assert!(
        r.attach_page(&pa, ib, super::tests::config())
            .await
            .is_err()
    );
    r.shutdown().unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), ta.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    assert_eq!(
        timeout(Duration::from_secs(1), tb.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop((da, db));
}

#[tokio::test]
async fn refresh_keeps_owned_session_but_old_foreign_closed_and_shutdown_pages_fail() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let old = r.activate_launch_page(a).unwrap();
    let (h, task, driver) = session();
    let id = r.register(a, h).unwrap();
    let fresh = r.activate_launch_page(a).unwrap();
    assert!(r.owned_page_sessions(&old, &[id]).is_err());
    assert_eq!(r.owned_page_sessions(&fresh, &[id]).unwrap(), vec![id]);
    let foreign = SessionRegistry::new();
    assert!(foreign.owned_page_sessions(&fresh, &[id]).is_err());
    r.close_window(a).unwrap();
    assert!(r.owned_page_sessions(&fresh, &[id]).is_err());
    r.shutdown().unwrap();
    assert!(r.owned_page_sessions(&fresh, &[]).is_err());
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop(driver);
}

#[tokio::test]
async fn only_existing_explicit_transfer_changes_the_display_owner() {
    let r = SessionRegistry::new();
    let a = r.open_window().unwrap();
    let b = r.open_window().unwrap();
    let pa = r.activate_launch_page(a).unwrap();
    let pb = r.activate_launch_page(b).unwrap();
    let (h, task, driver) = session();
    let id = r.register(a, h).unwrap();
    assert!(r.owned_page_sessions(&pb, &[id]).unwrap().is_empty());
    r.transfer(a, b, id).unwrap();
    assert!(r.owned_page_sessions(&pa, &[id]).unwrap().is_empty());
    assert_eq!(r.owned_page_sessions(&pb, &[id]).unwrap(), vec![id]);
    r.shutdown().unwrap();
    assert_eq!(
        timeout(Duration::from_secs(1), task.join())
            .await
            .unwrap()
            .unwrap()
            .lifecycle,
        Lifecycle::Closed
    );
    drop(driver);
}
