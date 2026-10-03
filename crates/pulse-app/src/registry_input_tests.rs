#![allow(clippy::unwrap_used)]
use super::*;
#[tokio::test]
async fn generation_bound_input_denies_foreign_old_and_revoked_views() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let stranger = registry.open_window().unwrap();
    let (handle, task, mut driver) = session();
    let id = registry.register(owner, handle).unwrap();
    let mut first = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut first).await;
    assert!(
        registry
            .input_bound(stranger, id, first.generation, b"foreign".to_vec())
            .await
            .is_err()
    );
    assert!(
        registry
            .input_bound(owner, id, first.generation + 1, b"stale".to_vec())
            .await
            .is_err()
    );
    registry
        .input_bound(
            owner,
            id,
            first.generation,
            "typed中文🌍\r".as_bytes().to_vec(),
        )
        .await
        .unwrap();
    assert!(
        matches!(driver.input.recv().await,Some(pulse_term::session::TransportWrite::Input(b)) if b=="typed中文🌍\r".as_bytes())
    );
    let mut next = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut next).await;
    assert!(
        registry
            .input_bound(owner, id, first.generation, b"old".to_vec())
            .await
            .is_err()
    );
    registry.detach(owner, id, next.generation).unwrap();
    assert!(
        registry
            .input_bound(owner, id, next.generation, b"revoked".to_vec())
            .await
            .is_err()
    );
    assert!(driver.input.try_recv().is_err());
    registry.close_session(owner, id).unwrap();
    task.join().await.unwrap();
    let _ = first.completion.await;
    let _ = next.completion.await;
}
