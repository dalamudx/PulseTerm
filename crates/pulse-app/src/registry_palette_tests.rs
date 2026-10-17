#![allow(clippy::unwrap_used)]
use super::super::tests::{config, session};
use super::*;
use pulse_term::QueryPalette;

#[tokio::test]
async fn palette_capture_is_exact_for_both_protocols_and_old_leases_are_rejected() {
    for dual in [false, true] {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let foreign = registry.open_window().unwrap();
        let (handle, task, driver) = session();
        let a = QueryPalette {
            foreground: [1, 2, 3],
            ..QueryPalette::default()
        };
        handle.set_query_palette(a.clone()).await.unwrap();
        let id = registry.register(owner, handle.clone()).unwrap();
        let attach = if dual {
            registry.attach_v2(owner, id, config()).await.unwrap()
        } else {
            registry.attach(owner, id, config()).await.unwrap()
        };
        let capture = registry
            .captured_query_palette(owner, id, attach.generation)
            .unwrap();
        assert_eq!(capture.palette, a);
        assert_eq!(capture.generation, attach.generation);
        assert!(matches!(
            registry.captured_query_palette(foreign, id, attach.generation),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.captured_query_palette(owner, id, attach.generation + 1),
            Err(RegistryError::Stale)
        ));
        let other = SessionRegistry::new();
        assert!(matches!(
            other.captured_query_palette(owner, id, attach.generation),
            Err(RegistryError::Forbidden)
        ));
        let b = QueryPalette {
            background: [4, 5, 6],
            ..a.clone()
        };
        handle.set_query_palette(b.clone()).await.unwrap();
        assert!(matches!(
            registry.captured_query_palette(owner, id, attach.generation),
            Err(RegistryError::Stale)
        ));
        assert_eq!(capture.palette, a);
        let fresh = if dual {
            registry.attach_v2(owner, id, config()).await.unwrap()
        } else {
            registry.attach(owner, id, config()).await.unwrap()
        };
        let current = registry
            .captured_query_palette(owner, id, fresh.generation)
            .unwrap();
        assert_eq!(current.palette, b);
        assert!(current.boundary.revision > capture.boundary.revision);
        assert_eq!(current.boundary.parsed_bytes, capture.boundary.parsed_bytes);
        registry.detach(owner, id, fresh.generation).unwrap();
        assert!(matches!(
            registry.captured_query_palette(owner, id, fresh.generation),
            Err(RegistryError::Stale)
        ));
        assert_eq!(
            handle.state.borrow().lifecycle,
            pulse_term::session::Lifecycle::Running
        );
        registry.shutdown().unwrap();
        drop(driver);
        task.join().await.unwrap();
    }
}

#[tokio::test]
async fn closed_window_cannot_read_palette_and_session_survives() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle.clone()).unwrap();
    let attach = registry.attach(owner, id, config()).await.unwrap();
    registry.close_window(owner).unwrap();
    assert!(matches!(
        registry.captured_query_palette(owner, id, attach.generation),
        Err(RegistryError::Forbidden)
    ));
    assert_eq!(
        handle.state.borrow().lifecycle,
        pulse_term::session::Lifecycle::Running
    );
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
