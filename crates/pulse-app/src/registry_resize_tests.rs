#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn bound_resize_denies_foreign_then_revokes_old_and_reattaches_same_session() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let stranger = registry.open_window().unwrap();
    let (handle, task, mut driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut first = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut first).await;
    let next = TermSize::new(60, 16).unwrap();
    assert!(
        registry
            .resize_bound(stranger, id, first.generation, next)
            .await
            .is_err()
    );
    assert!(
        registry
            .resize_bound(owner, id, first.generation + 1, next)
            .await
            .is_err()
    );
    assert!(driver.input.try_recv().is_err());
    let mut request = Box::pin(registry.resize_bound(owner, id, first.generation, next));
    let event = tokio::select! {r=&mut request=>panic!("premature resize: {r:?}"),e=driver.input.recv()=>e.unwrap()};
    if let pulse_term::session::TransportWrite::Resize(_, ack) = event {
        ack.send(Ok(())).unwrap();
    }
    request.await.unwrap();
    assert_eq!(registry.state(owner, id).unwrap().size, next);
    assert!(matches!(
        first.completion.await.unwrap(),
        Err(AttachmentError::Invalidated(
            pulse_term::session::StreamEnd::Reconfigured
        ))
    ));
    let mut rebound = registry.attach(owner, id, config()).await.unwrap();
    assert!(rebound.generation > first.generation);
    bootstrap(&registry, owner, id, &mut rebound).await;
    assert!(
        registry
            .resize_bound(owner, id, first.generation, next)
            .await
            .is_err()
    );
    assert!(
        registry
            .input_bound(owner, id, first.generation, b"late".to_vec())
            .await
            .is_err()
    );
    assert!(
        registry
            .acknowledge(owner, id, first.generation, 0)
            .is_err()
    );
    assert!(registry.detach(owner, id, first.generation).is_err());
    assert!(driver.input.try_recv().is_err());
    registry
        .input_bound(owner, id, rebound.generation, b"current".to_vec())
        .await
        .unwrap();
    assert!(
        matches!(driver.input.recv().await,Some(pulse_term::session::TransportWrite::Input(b)) if b==b"current")
    );
    registry.close_session(owner, id).unwrap();
    task.join().await.unwrap();
    let _ = rebound.completion.await;
}
