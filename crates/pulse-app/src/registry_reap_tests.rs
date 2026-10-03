#![allow(clippy::unwrap_used)]
use super::*;
use pulse_term::session::{self, ChannelTransport};

#[tokio::test]
async fn only_closed_actors_can_be_reaped_after_window_revocation() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    let id = registry.register(owner, handle.clone()).unwrap();
    registry.close_window(owner).unwrap();
    assert!(matches!(registry.reap_closed(id), Err(RegistryError::Busy)));
    assert_eq!(registry.lock().unwrap().sessions.len(), 1);
    handle.close();
    drop(driver);
    task.join().await.unwrap();
    registry.reap_closed(id).unwrap();
    assert!(registry.lock().unwrap().sessions.is_empty());
    assert!(matches!(
        registry.reap_closed(id),
        Err(RegistryError::Forbidden)
    ));
}

#[tokio::test]
async fn repeated_revoked_probes_do_not_exhaust_registry_slots() {
    let registry = SessionRegistry::new();
    for _ in 0..70 {
        let owner = registry.open_window().unwrap();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
        let id = registry.register(owner, handle.clone()).unwrap();
        registry.close_window(owner).unwrap();
        handle.close();
        drop(driver);
        task.join().await.unwrap();
        registry.reap_closed(id).unwrap();
    }
    assert!(registry.lock().unwrap().sessions.is_empty());
    assert!(registry.lock().unwrap().windows.is_empty());
}
