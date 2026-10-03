#![allow(clippy::unwrap_used)]
use super::*;
use pulse_term::session::{self, ChannelTransport, DriverEndpoint, Lifecycle, SessionTask};
use std::time::Duration;
use tokio::time::timeout;
pub(super) fn session() -> (SessionHandle, SessionTask, DriverEndpoint) {
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = session::spawn_session(TermSize::new(32, 8).unwrap(), 64, transport);
    (handle, task, driver)
}
pub(super) fn config() -> AttachmentConfig {
    AttachmentConfig {
        fragment: 1024,
        frames: 1,
        bytes: 1100,
        ack_timeout: Duration::from_secs(2),
    }
}
pub(super) async fn bootstrap(
    registry: &SessionRegistry,
    owner: WindowToken,
    id: SessionId,
    binding: &mut RegisteredAttachment,
) {
    loop {
        let wire = timeout(Duration::from_secs(1), binding.wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach::decode_fragment(frame.payload)
            .unwrap()
            .part;
        registry
            .acknowledge(owner, id, binding.generation, frame.sequence)
            .unwrap();
        if part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
}
#[tokio::test]
async fn foreign_window_and_foreign_registry_cannot_act_on_session() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let stranger = registry.open_window().unwrap();
    let foreign = SessionRegistry::new().open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    for token in [stranger, foreign] {
        assert!(matches!(
            registry.resolve(token, id.get()),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.input(token, id, b"bad".to_vec()).await,
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.attach(token, id, config()).await,
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.acknowledge(token, id, 1, 0),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            registry.close_session(token, id),
            Err(RegistryError::Forbidden)
        ));
    }
    assert!(matches!(
        registry.resolve(owner, u64::MAX),
        Err(RegistryError::Forbidden)
    ));
    registry.close_session(owner, id).unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn stale_ack_detach_and_old_completion_cannot_touch_replacement() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut old = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut old).await;
    let mut new = registry.attach(owner, id, config()).await.unwrap();
    assert!(new.generation > old.generation);
    assert!(matches!(
        registry.acknowledge(owner, id, old.generation, 0),
        Err(RegistryError::Stale)
    ));
    assert!(matches!(
        registry.detach(owner, id, old.generation),
        Err(RegistryError::Stale)
    ));
    assert!(old.completion.await.unwrap().is_err());
    bootstrap(&registry, owner, id, &mut new).await;
    registry.detach(owner, id, new.generation).unwrap();
    assert!(new.completion.await.unwrap().is_err());
    assert_eq!(
        registry.state(owner, id).unwrap().lifecycle,
        Lifecycle::Running
    );
    registry.close_session(owner, id).unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn cancellation_before_actor_capture_preserves_current_generation() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut binding = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut binding).await;
    {
        let capture = registry.attach(owner, id, config());
        fn require_send<T: Send>(_: &T) {}
        require_send(&capture);
        tokio::pin!(capture);
        tokio::select! {biased;_=&mut capture=>panic!("capture unexpectedly ready"),_=std::future::ready(())=>{}}
    }
    tokio::task::yield_now().await;
    assert_eq!(
        registry
            .lock()
            .unwrap()
            .bindings
            .get(&id)
            .unwrap()
            .generation,
        binding.generation
    );
    driver.output.send(b"alive".to_vec()).await.unwrap();
    let wire = timeout(Duration::from_secs(1), binding.wire.recv())
        .await
        .unwrap()
        .unwrap();
    let frame = pulse_ipc::decode(&wire).unwrap();
    assert_eq!(
        pulse_ipc::attach::decode_fragment(frame.payload)
            .unwrap()
            .part,
        pulse_ipc::attach::Part::Data
    );
    registry
        .acknowledge(owner, id, binding.generation, frame.sequence)
        .unwrap();
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn duplicate_actor_and_forged_watch_cannot_create_second_owner() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let mut clone = handle.clone();
    let id = registry.register(owner, handle).unwrap();
    let (_, fake_watch) = tokio::sync::watch::channel(registry.state(owner, id).unwrap());
    clone.state = fake_watch;
    assert!(matches!(
        registry.register(owner, clone.clone()),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry.register(other, clone),
        Err(RegistryError::Forbidden)
    ));
    let (other_handle, other_task, other_driver) = session();
    assert!(registry.register(other, other_handle).is_ok());
    registry.shutdown().unwrap();
    drop(driver);
    drop(other_driver);
    task.join().await.unwrap();
    other_task.join().await.unwrap();
}
#[tokio::test]
async fn failed_and_busy_capture_preserve_current_binding() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut current = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut current).await;
    assert!(matches!(
        registry
            .attach(
                owner,
                id,
                AttachmentConfig {
                    frames: 0,
                    ..config()
                }
            )
            .await,
        Err(RegistryError::Attachment(_))
    ));
    let gate = registry
        .lock()
        .unwrap()
        .sessions
        .get(&id)
        .unwrap()
        .capture
        .clone();
    let _guard = gate.try_lock_owned().unwrap();
    assert!(matches!(
        registry.attach(owner, id, config()).await,
        Err(RegistryError::Busy)
    ));
    assert_eq!(
        registry
            .lock()
            .unwrap()
            .bindings
            .get(&id)
            .unwrap()
            .generation,
        current.generation
    );
    registry.detach(owner, id, current.generation).unwrap();
    registry.close_session(owner, id).unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn close_window_detaches_without_closing_and_recreation_cannot_inherit() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let observer = handle.clone();
    let id = registry.register(owner, handle).unwrap();
    let mut binding = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut binding).await;
    registry.close_window(owner).unwrap();
    assert!(binding.completion.await.unwrap().is_err());
    let recreated = registry.open_window().unwrap();
    assert_ne!(owner, recreated);
    assert!(matches!(
        registry.resolve(recreated, id.get()),
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry.resolve(owner, id.get()),
        Err(RegistryError::Forbidden)
    ));
    assert_eq!(observer.state.borrow().lifecycle, Lifecycle::Running);
    driver.output.send(b"still alive".to_vec()).await.unwrap();
    timeout(Duration::from_secs(1), async {
        while observer.state.borrow().parsed_bytes < 11 {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn window_close_during_capture_cannot_resurrect_view() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let observer = handle.clone();
    let id = registry.register(owner, handle).unwrap();
    let capture = registry.attach(owner, id, config());
    tokio::pin!(capture);
    tokio::select! {biased; _=&mut capture=>panic!("actor capture unexpectedly ready before actor ran"),_=std::future::ready(())=>{}}
    registry.close_window(owner).unwrap();
    assert!(matches!(capture.await, Err(RegistryError::Forbidden)));
    assert!(registry.lock().unwrap().bindings.is_empty());
    assert_eq!(observer.state.borrow().lifecycle, Lifecycle::Running);
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn normal_eof_cleans_binding_only_after_final_ack() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut binding = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut binding).await;
    driver.output.send(vec![b'a', 0xf0, 0x9f]).await.unwrap();
    drop(driver);
    loop {
        let wire = timeout(Duration::from_secs(1), binding.wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach::decode_fragment(frame.payload)
            .unwrap()
            .part;
        assert!(registry.lock().unwrap().bindings.contains_key(&id));
        registry
            .acknowledge(owner, id, binding.generation, frame.sequence)
            .unwrap();
        if part == pulse_ipc::attach::Part::End {
            break;
        }
    }
    assert_eq!(
        binding
            .completion
            .await
            .unwrap()
            .unwrap()
            .boundary
            .parsed_bytes,
        3
    );
    assert!(!registry.lock().unwrap().bindings.contains_key(&id));
    assert_eq!(
        registry.state(owner, id).unwrap().lifecycle,
        Lifecycle::Ended
    );
    task.join().await.unwrap();
    registry.close_session(owner, id).unwrap();
}
#[tokio::test]
async fn application_drop_closes_sessions_and_revokes_binding() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut binding = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut binding).await;
    drop(registry);
    assert!(binding.completion.await.unwrap().is_err());
    drop(driver);
    assert_eq!(task.join().await.unwrap().lifecycle, Lifecycle::Closed);
}
#[tokio::test]
async fn monitor_limit_rejects_before_replacing_binding() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut current = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut current).await;
    let permits = registry
        .monitors
        .clone()
        .try_acquire_many_owned(63)
        .unwrap();
    assert!(matches!(
        registry.attach(owner, id, config()).await,
        Err(RegistryError::Busy)
    ));
    assert_eq!(
        registry
            .lock()
            .unwrap()
            .bindings
            .get(&id)
            .unwrap()
            .generation,
        current.generation
    );
    drop(permits);
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
#[tokio::test]
async fn session_count_and_id_exhaustion_are_bounded() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let mut workers = Vec::new();
    for _ in 0..64 {
        let (handle, task, driver) = session();
        registry.register(owner, handle).unwrap();
        workers.push((task, driver));
    }
    let (handle, task, driver) = session();
    assert!(matches!(
        registry.register(owner, handle.clone()),
        Err(RegistryError::Busy)
    ));
    handle.close();
    drop(driver);
    task.join().await.unwrap();
    registry.shutdown().unwrap();
    for (task, driver) in workers {
        drop(driver);
        task.join().await.unwrap();
    }
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    registry.lock().unwrap().next_session = u64::MAX;
    assert!(matches!(
        registry.register(owner, handle.clone()),
        Err(RegistryError::Unavailable)
    ));
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Running);
    handle.close();
    drop(driver);
    task.join().await.unwrap();
}
#[test]
fn window_limit_and_shutdown_fail_closed() {
    let registry = SessionRegistry::new();
    let tokens: Vec<_> = (0..32).map(|_| registry.open_window().unwrap()).collect();
    assert!(matches!(registry.open_window(), Err(RegistryError::Busy)));
    registry.close_window(tokens[0]).unwrap();
    assert_ne!(tokens[0], registry.open_window().unwrap());
    registry.shutdown().unwrap();
    assert!(matches!(
        registry.open_window(),
        Err(RegistryError::Unavailable)
    ));
}
#[tokio::test]
async fn identifier_exhaustion_preserves_existing_binding() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let (handle, task, driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut current = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut current).await;
    registry.lock().unwrap().next_generation = u64::MAX;
    assert!(matches!(
        registry.attach(owner, id, config()).await,
        Err(RegistryError::Unavailable)
    ));
    assert_eq!(
        registry
            .lock()
            .unwrap()
            .bindings
            .get(&id)
            .unwrap()
            .generation,
        current.generation
    );
    registry.shutdown().unwrap();
    drop(driver);
    task.join().await.unwrap();
}
