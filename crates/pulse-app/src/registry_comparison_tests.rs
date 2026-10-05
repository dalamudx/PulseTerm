#![allow(clippy::unwrap_used)]
use super::super::tests::{bootstrap, config, session};
use super::*;
#[tokio::test]
async fn repair_admission_is_authorized_generation_scoped_and_rate_bound() {
    let registry = SessionRegistry::new();
    let owner = registry.open_window().unwrap();
    let other = registry.open_window().unwrap();
    let (h, task, d) = session();
    let id = registry.register(owner, h.clone()).unwrap();
    let mut b = registry.attach(owner, id, config()).await.unwrap();
    bootstrap(&registry, owner, id, &mut b).await;
    let captured = h.diagnostic_digest().await.unwrap();
    let rendered = RenderedScreen {
        boundary: captured.boundary,
        digest: captured.digest,
    };
    assert!(matches!(
        registry
            .request_repair(other, id, b.generation, rendered.clone())
            .await,
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        registry
            .request_repair(owner, id, b.generation + 1, rendered.clone())
            .await,
        Err(RegistryError::Stale)
    ));
    assert_eq!(
        registry
            .request_repair(owner, id, b.generation, rendered.clone())
            .await
            .unwrap(),
        RepairAdmission::Equal
    );
    assert!(matches!(
        registry
            .request_repair(owner, id, b.generation, rendered.clone())
            .await,
        Err(RegistryError::Busy)
    ));
    tokio::time::sleep(MIN_INTERVAL).await;
    let mut bad = rendered.clone();
    bad.digest = "0000000000000000".into();
    let pending = registry.request_repair(owner, id, b.generation, bad);
    tokio::pin!(pending);
    tokio::select! {biased; _=&mut pending=>panic!("request should await actor"),_=std::future::ready(())=>{}}
    let mut replacement = registry.attach(owner, id, config()).await.unwrap();
    assert!(matches!(pending.await, Err(RegistryError::Stale)));
    bootstrap(&registry, owner, id, &mut replacement).await;
    assert!(matches!(
        registry
            .request_repair(owner, id, replacement.generation, rendered)
            .await,
        Err(RegistryError::Busy)
    ));
    registry.close_window(owner).unwrap();
    assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
    h.close();
    drop(d);
    task.join().await.unwrap();
}

use pulse_term::session::{DriverEndpoint, SessionHandle, SessionTask};
#[tokio::test]
async fn marker_request_is_authorized_rate_limited_and_cannot_cross_replacement() {
    let f = Fixture::new().await;
    let other = f.registry.open_window().unwrap();
    assert!(matches!(
        f.registry
            .request_marker(other, f.id, f.binding.generation)
            .await,
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        f.registry
            .request_marker(f.owner, f.id, f.binding.generation + 1)
            .await,
        Err(RegistryError::Stale)
    ));
    let registry = f.registry.clone();
    {
        let pending = registry.request_marker(f.owner, f.id, f.binding.generation);
        tokio::pin!(pending);
        tokio::select! { biased; _=&mut pending=>panic!("unexpected synchronous completion"),_=std::future::ready(())=>{} }
        let mut replacement = f.registry.attach(f.owner, f.id, config()).await.unwrap();
        assert!(matches!(pending.await, Err(RegistryError::Stale)));
        bootstrap(&f.registry, f.owner, f.id, &mut replacement).await;
        assert!(matches!(
            f.registry
                .request_marker(f.owner, f.id, replacement.generation)
                .await,
            Err(RegistryError::Busy)
        ));
        tokio::time::sleep(MIN_INTERVAL).await;
        f.registry
            .request_marker(f.owner, f.id, replacement.generation)
            .await
            .unwrap();
        let wire = replacement.wire.recv().await.unwrap();
        let frame = pulse_ipc::decode(&wire).unwrap();
        assert_eq!(
            pulse_ipc::attach::decode_fragment(frame.payload)
                .unwrap()
                .part,
            pulse_ipc::attach::Part::Marker
        );
        assert!(matches!(
            f.registry
                .request_marker(f.owner, f.id, replacement.generation)
                .await,
            Err(RegistryError::Busy)
        ));
        f.registry
            .acknowledge(f.owner, f.id, replacement.generation, frame.sequence)
            .unwrap();
    }
    f.close().await;
}
struct Fixture {
    registry: SessionRegistry,
    owner: WindowToken,
    id: SessionId,
    handle: SessionHandle,
    task: SessionTask,
    driver: DriverEndpoint,
    binding: RegisteredAttachment,
}
impl Fixture {
    async fn new() -> Self {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let (handle, task, driver) = session();
        let input = b"q\x1b[2b";
        driver.output.send(input.to_vec()).await.unwrap();
        let mut state = handle.state.clone();
        while state.borrow().parsed_bytes < input.len() as u64 {
            state.changed().await.unwrap();
        }
        let id = registry.register(owner, handle.clone()).unwrap();
        let mut binding = registry.attach(owner, id, config()).await.unwrap();
        bootstrap(&registry, owner, id, &mut binding).await;
        Self {
            registry,
            owner,
            id,
            handle,
            task,
            driver,
            binding,
        }
    }
    async fn rendered(&self) -> RenderedScreen {
        let captured = self.handle.diagnostic_digest().await.unwrap();
        RenderedScreen {
            boundary: captured.boundary,
            digest: captured.digest,
        }
    }
    async fn close(self) {
        self.registry.close_session(self.owner, self.id).unwrap();
        drop(self.driver);
        self.task.join().await.unwrap();
    }
}
#[tokio::test]
async fn equality_and_mismatch_never_clear_risk_and_are_rate_limited() {
    let f = Fixture::new().await;
    let before = f.handle.diagnostic_digest().await.unwrap().risk;
    assert_ne!(before, 0);
    let rendered = f.rendered().await;
    assert_eq!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered.clone())
            .await
            .unwrap(),
        VisibleComparison::Equal
    );
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered.clone())
            .await,
        Err(RegistryError::Busy)
    ));
    tokio::time::sleep(MIN_INTERVAL).await;
    let mut wrong = rendered;
    wrong.digest = "0000000000000000".into();
    assert_eq!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, wrong)
            .await
            .unwrap(),
        VisibleComparison::Mismatch
    );
    assert_eq!(f.handle.diagnostic_digest().await.unwrap().risk, before);
    f.close().await;
}
#[tokio::test]
async fn foreign_stale_and_invalid_requests_do_not_spend_admission() {
    let f = Fixture::new().await;
    let other = f.registry.open_window().unwrap();
    let rendered = f.rendered().await;
    assert!(matches!(
        f.registry
            .compare_visible(other, f.id, f.binding.generation, rendered.clone())
            .await,
        Err(RegistryError::Forbidden)
    ));
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation + 1, rendered.clone())
            .await,
        Err(RegistryError::Stale)
    ));
    let mut bad = rendered.clone();
    bad.digest = "BAD".into();
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, bad)
            .await,
        Err(RegistryError::Session(SessionError::InvalidDigest))
    ));
    assert_eq!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered)
            .await
            .unwrap(),
        VisibleComparison::Equal
    );
    f.close().await;
}
#[tokio::test]
async fn only_one_comparison_is_admitted_per_session() {
    let f = Fixture::new().await;
    let rendered = f.rendered().await;
    let entry = SessionRegistry::entry(&f.registry.lock().unwrap(), f.owner, f.id).unwrap();
    let admission = entry.comparison.lock().await;
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered.clone())
            .await,
        Err(RegistryError::Busy)
    ));
    drop(admission);
    assert_eq!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered)
            .await
            .unwrap(),
        VisibleComparison::Equal
    );
    f.close().await;
}
#[tokio::test]
async fn actor_advance_is_stale_not_a_mismatch() {
    let f = Fixture::new().await;
    let rendered = f.rendered().await;
    f.driver.output.send(b"next".to_vec()).await.unwrap();
    let mut state = f.handle.state.clone();
    while state.borrow().parsed_bytes == rendered.boundary.parsed_bytes {
        state.changed().await.unwrap();
    }
    assert_eq!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered)
            .await
            .unwrap(),
        VisibleComparison::Stale
    );
    f.close().await;
}
#[tokio::test]
async fn replacement_during_comparison_rejects_old_evidence() {
    let f = Fixture::new().await;
    let rendered = f.rendered().await;
    let registry = f.registry.clone();
    let comparison =
        registry.compare_visible(f.owner, f.id, f.binding.generation, rendered.clone());
    tokio::pin!(comparison);
    tokio::select! {biased; _=&mut comparison=>panic!("unexpected synchronous completion"),_=std::future::ready(())=>{}}
    let mut replacement = f.registry.attach(f.owner, f.id, config()).await.unwrap();
    assert!(matches!(comparison.await, Err(RegistryError::Stale)));
    bootstrap(&f.registry, f.owner, f.id, &mut replacement).await;
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, replacement.generation, rendered)
            .await,
        Err(RegistryError::Busy)
    ));
    f.close().await;
}
#[tokio::test]
async fn cancellation_keeps_cooldown_and_never_clears_hints() {
    let f = Fixture::new().await;
    let rendered = f.rendered().await;
    let before = f.handle.diagnostic_digest().await.unwrap().risk;
    {
        let comparison =
            f.registry
                .compare_visible(f.owner, f.id, f.binding.generation, rendered.clone());
        tokio::pin!(comparison);
        tokio::select! {biased; _=&mut comparison=>panic!("unexpected synchronous completion"),_=std::future::ready(())=>{}}
    }
    assert!(matches!(
        f.registry
            .compare_visible(f.owner, f.id, f.binding.generation, rendered)
            .await,
        Err(RegistryError::Busy)
    ));
    assert_eq!(f.handle.diagnostic_digest().await.unwrap().risk, before);
    f.close().await;
}
#[tokio::test]
async fn window_revocation_wins_over_pending_evidence() {
    let f = Fixture::new().await;
    let rendered = f.rendered().await;
    let comparison = f
        .registry
        .compare_visible(f.owner, f.id, f.binding.generation, rendered);
    tokio::pin!(comparison);
    tokio::select! {biased; _=&mut comparison=>panic!("unexpected synchronous completion"),_=std::future::ready(())=>{}}
    f.registry.close_window(f.owner).unwrap();
    assert!(matches!(comparison.await, Err(RegistryError::Forbidden)));
    f.handle.close();
    drop(f.driver);
    f.task.join().await.unwrap();
}
#[tokio::test]
async fn output_during_bootstrap_keeps_capture_exact_but_latest_confirmation_stale() {
    use pulse_ipc::attach::Part;
    use pulse_term::TerminalProbe;
    for dual in [false, true] {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let (h, task, d) = session();
        let id = registry.register(owner, h.clone()).unwrap();
        let mut observed = h.state.clone();
        d.output.send(b"a".to_vec()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while observed.borrow().parsed_bytes < 1 {
                observed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let captured = h.diagnostic_digest().await.unwrap();
        let mut binding = if dual {
            registry.attach_v2(owner, id, config()).await.unwrap()
        } else {
            registry.attach(owner, id, config()).await.unwrap()
        };
        // No bootstrap ACK yet. Real actor advances and queues post-boundary output.
        d.output.send(b"b".to_vec()).await.unwrap();
        tokio::time::timeout(std::time::Duration::from_secs(1), async {
            while observed.borrow().parsed_bytes < 2 {
                observed.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let latest = h.diagnostic_digest().await.unwrap();
        assert_ne!(latest.boundary, captured.boundary);
        let mut restore = Vec::new();
        loop {
            let bytes =
                tokio::time::timeout(std::time::Duration::from_secs(1), binding.wire.recv())
                    .await
                    .unwrap()
                    .unwrap();
            let frame = pulse_ipc::decode(&bytes).unwrap();
            let (part, body) = if dual {
                let p = pulse_ipc::attach_v2::decode_fragment(frame.payload).unwrap();
                (p.part, p.body)
            } else {
                let p = pulse_ipc::attach::decode_fragment(frame.payload).unwrap();
                (p.part, p.body)
            };
            match part {
                Part::Begin => {
                    if dual {
                        let (b, _, _) = pulse_ipc::attach_v2::decode_begin(body).unwrap();
                        assert_eq!(b.source_bytes, captured.boundary.parsed_bytes);
                        assert_eq!(b.utf8_bytes, captured.boundary.utf8_bytes);
                    } else {
                        let (b, _, _) = pulse_ipc::attach::decode_begin(body).unwrap();
                        assert_eq!(b.parsed_bytes, captured.boundary.parsed_bytes);
                    }
                }
                Part::Restore => restore.extend_from_slice(body),
                Part::Commit => {}
                _ => panic!("unexpected bootstrap part"),
            };
            registry
                .acknowledge(owner, id, binding.generation, frame.sequence)
                .unwrap();
            if part == Part::Commit {
                break;
            }
        }
        let mut replay = TerminalProbe::new(captured.boundary.size, 64);
        replay.feed(&restore);
        assert_eq!(replay.screen_digest(), captured.digest);
        assert_eq!(
            registry
                .compare_visible(
                    owner,
                    id,
                    binding.generation,
                    RenderedScreen {
                        boundary: captured.boundary,
                        digest: replay.screen_digest()
                    }
                )
                .await
                .unwrap(),
            VisibleComparison::Stale
        );
        let bytes = tokio::time::timeout(std::time::Duration::from_secs(1), binding.wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&bytes).unwrap();
        let (part, offset, body) = if dual {
            let p = pulse_ipc::attach_v2::decode_fragment(frame.payload).unwrap();
            (p.part, p.offset, p.body)
        } else {
            let p = pulse_ipc::attach::decode_fragment(frame.payload).unwrap();
            (p.part, p.offset, p.body)
        };
        assert_eq!(part, Part::Data);
        assert_eq!(offset, 1);
        assert_eq!(body, b"b");
        replay.feed(body);
        registry
            .acknowledge(owner, id, binding.generation, frame.sequence)
            .unwrap();
        assert_eq!(replay.screen_digest(), latest.digest);
        tokio::time::sleep(MIN_INTERVAL).await;
        assert_eq!(
            registry
                .compare_visible(
                    owner,
                    id,
                    binding.generation,
                    RenderedScreen {
                        boundary: latest.boundary,
                        digest: replay.screen_digest()
                    }
                )
                .await
                .unwrap(),
            VisibleComparison::Equal
        );
        registry.close_session(owner, id).unwrap();
        assert_eq!(
            tokio::time::timeout(std::time::Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((binding, d));
    }
}
