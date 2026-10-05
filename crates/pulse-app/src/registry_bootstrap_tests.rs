#![allow(clippy::unwrap_used)]
use super::super::tests::{config, session};
use super::*;
use pulse_term::session::Lifecycle;
#[tokio::test]
async fn slow_bootstrap_keeps_stream_bounded_and_cached_confirmation_valid_while_output_continues()
{
    use pulse_ipc::attach::Part;
    use pulse_term::TerminalProbe;
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let (h, task, d) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let captured = h.diagnostic_digest().await.unwrap();
        let mut b = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        let first = tokio::time::timeout(Duration::from_secs(1), b.wire.recv())
            .await
            .unwrap()
            .unwrap();
        let frame = pulse_ipc::decode(&first).unwrap();
        assert_eq!(frame.sequence, 0);
        let block = "x中🌍".repeat(512).into_bytes();
        let expected = block.repeat(64);
        let total = expected.len() as u64;
        let tx = d.output.clone();
        let producer = tokio::spawn(async move {
            for _ in 0..64 {
                tx.send(block.clone()).await.unwrap();
            }
        });
        tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                let before = h.state.borrow().parsed_bytes;
                if before > 0 {
                    tokio::time::sleep(Duration::from_millis(30)).await;
                    if h.state.borrow().parsed_bytes == before {
                        break;
                    }
                } else {
                    tokio::task::yield_now().await;
                }
            }
        })
        .await
        .unwrap();
        let paused = h.state.borrow().clone();
        assert!(paused.parsed_bytes > captured.boundary.parsed_bytes);
        assert!(paused.parsed_bytes < total);
        assert!(!producer.is_finished());
        assert_eq!(
            r.compare_bootstrap(
                owner,
                id,
                b.generation,
                RenderedScreen {
                    boundary: captured.boundary.clone(),
                    digest: captured.digest.clone()
                }
            )
            .unwrap(),
            VisibleComparison::Equal
        );
        r.acknowledge(owner, id, b.generation, frame.sequence)
            .unwrap();
        let mut got = Vec::new();
        let mut restore = Vec::new();
        let mut bootstrap_checked = false;
        tokio::time::timeout(Duration::from_secs(8), async {
            loop {
                let bytes = b.wire.recv().await.unwrap();
                let frame = pulse_ipc::decode(&bytes).unwrap();
                let (part, offset, body) = if dual {
                    let p = pulse_ipc::attach_v2::decode_fragment(frame.payload).unwrap();
                    (p.part, p.offset, p.body)
                } else {
                    let p = pulse_ipc::attach::decode_fragment(frame.payload).unwrap();
                    (p.part, p.offset, p.body)
                };
                match part {
                    Part::Restore => restore.extend_from_slice(body),
                    Part::History => panic!("empty initial history"),
                    Part::Commit => {
                        let mut replay = TerminalProbe::new(captured.boundary.size, 64);
                        replay.feed(&restore);
                        assert_eq!(replay.screen_digest(), captured.digest);
                        bootstrap_checked = true;
                    }
                    Part::Data => {
                        assert!(bootstrap_checked);
                        assert_eq!(offset, got.len() as u64);
                        got.extend_from_slice(body);
                    }
                    _ => panic!("unexpected frame"),
                };
                r.acknowledge(owner, id, b.generation, frame.sequence)
                    .unwrap();
                if got.len() == expected.len() {
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(got, expected);
        producer.await.unwrap();
        let latest = h.diagnostic_digest().await.unwrap();
        assert_eq!(latest.boundary.parsed_bytes, total);
        let mut replay = TerminalProbe::new(latest.boundary.size, 64);
        replay.feed(&got);
        assert_eq!(replay.screen_digest(), latest.digest);
        r.check_binding(owner, id, b.generation).unwrap();
        r.close_session(owner, id).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((b, d));
    }
}

#[tokio::test]
async fn cached_confirmation_cannot_cross_replacement_or_transfer_and_keeps_shared_quota() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let target = r.open_window().unwrap();
        let (h, task, d) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let capture = h.diagnostic_digest().await.unwrap();
        let rendered = RenderedScreen {
            boundary: capture.boundary,
            digest: capture.digest,
        };
        let first = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        assert_eq!(
            r.compare_bootstrap(owner, id, first.generation, rendered.clone())
                .unwrap(),
            VisibleComparison::Equal
        );
        let second = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        assert!(matches!(
            r.compare_bootstrap(owner, id, first.generation, rendered.clone()),
            Err(RegistryError::Stale)
        ));
        assert!(matches!(
            r.compare_bootstrap(owner, id, second.generation, rendered.clone()),
            Err(RegistryError::Busy)
        ));
        r.transfer(owner, target, id).unwrap();
        assert!(matches!(
            r.compare_bootstrap(owner, id, second.generation, rendered.clone()),
            Err(RegistryError::Forbidden)
        ));
        let third = if dual {
            r.attach_v2(target, id, config()).await.unwrap()
        } else {
            r.attach(target, id, config()).await.unwrap()
        };
        assert!(matches!(
            r.compare_bootstrap(target, id, second.generation, rendered.clone()),
            Err(RegistryError::Stale)
        ));
        assert!(matches!(
            r.compare_bootstrap(target, id, third.generation, rendered.clone()),
            Err(RegistryError::Busy)
        ));
        tokio::time::sleep(MIN_INTERVAL).await;
        assert_eq!(
            r.compare_bootstrap(target, id, third.generation, rendered)
                .unwrap(),
            VisibleComparison::Equal
        );
        r.check_binding(target, id, third.generation).unwrap();
        r.close_session(target, id).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((first, second, third, d));
    }
}

#[tokio::test]
async fn captured_equal_does_not_clear_newer_nonzero_divergence_risk() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let (h, task, d) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let captured = h.diagnostic_digest().await.unwrap();
        let binding = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        let changed = b"q\x1b[2b\x1b[L";
        d.output.send(changed.to_vec()).await.unwrap();
        let mut watched = h.state.clone();
        tokio::time::timeout(Duration::from_secs(1), async {
            while watched.borrow().parsed_bytes < changed.len() as u64 {
                watched.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let before = h.diagnostic_digest().await.unwrap();
        assert_ne!(before.risk, 0);
        assert_eq!(
            r.compare_bootstrap(
                owner,
                id,
                binding.generation,
                RenderedScreen {
                    boundary: captured.boundary,
                    digest: captured.digest
                }
            )
            .unwrap(),
            VisibleComparison::Equal
        );
        let after = h.diagnostic_digest().await.unwrap();
        assert_eq!(after.risk, before.risk);
        assert_eq!(after.boundary, before.boundary);
        assert_eq!(after.digest, before.digest);
        r.close_session(owner, id).unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((binding, d));
    }
}

#[tokio::test]
async fn bootstrap_oracle_is_capture_bound_authorized_rate_shared_and_never_latest_evidence() {
    for dual in [false, true] {
        let r = SessionRegistry::new();
        let owner = r.open_window().unwrap();
        let foreign = r.open_window().unwrap();
        let (h, task, d) = session();
        let id = r.register(owner, h.clone()).unwrap();
        let mut watched = h.state.clone();
        d.output.send(b"a".to_vec()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while watched.borrow().parsed_bytes < 1 {
                watched.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let at = h.diagnostic_digest().await.unwrap();
        let binding = if dual {
            r.attach_v2(owner, id, config()).await.unwrap()
        } else {
            r.attach(owner, id, config()).await.unwrap()
        };
        d.output.send(b"b".to_vec()).await.unwrap();
        tokio::time::timeout(Duration::from_secs(1), async {
            while watched.borrow().parsed_bytes < 2 {
                watched.changed().await.unwrap();
            }
        })
        .await
        .unwrap();
        let latest = h.diagnostic_digest().await.unwrap();
        let rendered = RenderedScreen {
            boundary: at.boundary,
            digest: at.digest,
        };
        assert_ne!(rendered.boundary, latest.boundary);
        assert!(matches!(
            r.compare_bootstrap(foreign, id, binding.generation, rendered.clone()),
            Err(RegistryError::Forbidden)
        ));
        assert!(matches!(
            r.compare_bootstrap(owner, id, binding.generation + 1, rendered.clone()),
            Err(RegistryError::Stale)
        ));
        let mut invalid = rendered.clone();
        invalid.digest = "BAD".into();
        assert!(matches!(
            r.compare_bootstrap(owner, id, binding.generation, invalid),
            Err(RegistryError::Session(SessionError::InvalidDigest))
        ));
        assert_eq!(
            r.compare_bootstrap(owner, id, binding.generation, rendered.clone())
                .unwrap(),
            VisibleComparison::Equal
        );
        assert!(matches!(
            r.compare_visible(owner, id, binding.generation, rendered.clone())
                .await,
            Err(RegistryError::Busy)
        ));
        tokio::time::sleep(MIN_INTERVAL).await;
        assert_eq!(
            r.compare_visible(owner, id, binding.generation, rendered.clone())
                .await
                .unwrap(),
            VisibleComparison::Stale
        );
        assert!(matches!(
            r.compare_bootstrap(owner, id, binding.generation, rendered.clone()),
            Err(RegistryError::Busy)
        ));
        tokio::time::sleep(MIN_INTERVAL).await;
        let mut corrupt = rendered.clone();
        corrupt.digest = "0000000000000000".into();
        assert_ne!(corrupt.digest, rendered.digest);
        assert_eq!(
            r.compare_bootstrap(owner, id, binding.generation, corrupt)
                .unwrap(),
            VisibleComparison::Mismatch
        );
        tokio::time::sleep(MIN_INTERVAL).await;
        assert_eq!(
            r.compare_bootstrap(
                owner,
                id,
                binding.generation,
                RenderedScreen {
                    boundary: latest.boundary,
                    digest: latest.digest
                }
            )
            .unwrap(),
            VisibleComparison::Stale
        );
        r.close_window(owner).unwrap();
        assert!(matches!(
            r.compare_bootstrap(owner, id, binding.generation, rendered),
            Err(RegistryError::Forbidden)
        ));
        assert_eq!(h.state.borrow().lifecycle, Lifecycle::Running);
        h.close();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(1), task.join())
                .await
                .unwrap()
                .unwrap()
                .lifecycle,
            Lifecycle::Closed
        );
        drop((binding, d));
    }
}
