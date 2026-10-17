use super::*;
use pulse_term::session::{ChannelTransport, spawn_session};
use std::os::unix::fs::PermissionsExt;
#[tokio::test]
async fn actual_page_source_receipts_are_single_use_and_never_grant_foreign_or_stale_access()
-> std::result::Result<(), Box<dyn std::error::Error>> {
    let _serial = theme_probe::THEME_IO_TEST.lock().await;
    let dir = tempfile::tempdir()?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let (a, b, _) = theme_probe::files(dir.path().to_path_buf())?;
    let state = WindowSessions::default();
    let registry = &state.0.registry;
    let main = registry.open_window()?;
    let peer = registry.open_window()?;
    let foreign = registry.open_window()?;
    let (transport, driver) = ChannelTransport::bounded_driver();
    let (handle, task) = spawn_session(TermSize::new(40, 12)?, 128, transport);
    let id = registry.register(peer, handle)?;
    let binding = registry
        .attach(peer, id, AttachmentConfig::default())
        .await?;
    let generation = binding.generation;
    registry.configure_view_theme(peer, id, generation, a.handle())?;
    let c = registry
        .begin_view_theme(peer, id, generation)?
        .ready()
        .await?;
    let snapshot = registry.confirm_view_theme(peer, id, generation, c)?;
    let mut run = Active::new(
        main,
        "run".into(),
        id,
        Arc::new(crate::viewer_visibility::VisibilityGate::default()),
    );
    run.bind_peer(peer)?;
    run.peer_id = Some(id);
    let pages = run.peer_pages.clone();
    pages.record(tauri::webview::PageLoadEvent::Started);
    pages.record(tauri::webview::PageLoadEvent::Finished);
    let page = pages.injection()?.0;
    pages.confirm_nonce(&page)?;
    pages.claim_view(&page)?;
    pages.mark_captured(&page)?;
    *state.0.active.lock().map_err(|_| "active mutex")? = Some(run);
    assert!(read(&state, peer, "run", &page, generation).is_err());
    pages.mark_rendered(&page)?;
    assert!(read(&state, peer, "run", &page, generation).is_err()); // No trusted challenge.
    let (tx, rx) = oneshot::channel();
    let ticket = uuid::Uuid::new_v4();
    *state
        .0
        .theme_delivery
        .lock()
        .map_err(|_| "delivery mutex")? = Some(Challenge {
        owner: peer,
        id,
        generation,
        page: page.clone(),
        target_epoch: None,
        ticket,
        snapshot: snapshot.clone(),
        phase: 0,
        done: Some(tx),
        retirement: None,
    });
    for (owner, run_nonce, p, g) in [
        (foreign, "run", page.as_str(), generation),
        (main, "run", page.as_str(), generation),
        (peer, "wrong", page.as_str(), generation),
        (peer, "run", "bad", generation),
        (peer, "run", page.as_str(), generation + 1),
    ] {
        assert!(read(&state, owner, run_nonce, p, g).is_err());
    }
    let receipt = read(&state, peer, "run", &page, generation)?;
    assert_eq!(receipt.generation, generation.to_string());
    assert_eq!(
        receipt.paint_json,
        snapshot.theme().document().paint_data()?.to_json()?
    );
    assert!(receipt.paint_json.len() <= 4096 && !receipt.paint_json.contains("native-first"));
    assert!(read(&state, peer, "run", &page, generation).is_err());
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 2).is_err());
    assert!(
        advance(
            &state,
            foreign,
            "run",
            &page,
            generation,
            &receipt.ticket,
            1
        )
        .is_err()
    );
    assert!(advance(&state, peer, "run", &page, generation, "wrong", 1).is_err());
    advance(&state, peer, "run", &page, generation, &receipt.ticket, 1)?;
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 1).is_err());
    advance(&state, peer, "run", &page, generation, &receipt.ticket, 2)?;
    rx.await?;
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 2).is_err());
    // Same document and generation do not revive the old source receipt after A→B→A.
    state
        .0
        .theme_delivery
        .lock()
        .map_err(|_| "delivery mutex")?
        .as_mut()
        .ok_or("missing")?
        .phase = 1;
    registry.configure_view_theme(peer, id, generation, b.handle())?;
    registry.configure_view_theme(peer, id, generation, a.handle())?;
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 1).is_err());
    let c = registry
        .begin_view_theme(peer, id, generation)?
        .ready()
        .await?;
    registry.confirm_view_theme(peer, id, generation, c)?;
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 1).is_err());
    let old_guard = DeliveryGuard {
        inner: Arc::downgrade(&state.0),
        ticket,
    };
    let replacement = uuid::Uuid::new_v4();
    let (cancelled, cancelled_receipt) = oneshot::channel();
    {
        let mut slot = state
            .0
            .theme_delivery
            .lock()
            .map_err(|_| "delivery mutex")?;
        let c = slot.as_mut().ok_or("missing")?;
        c.ticket = replacement;
        c.done = Some(cancelled);
    }
    drop(old_guard);
    assert_eq!(
        state
            .0
            .theme_delivery
            .lock()
            .map_err(|_| "delivery mutex")?
            .as_ref()
            .ok_or("replacement")?
            .ticket,
        replacement
    );
    let cleanup = DeliveryGuard {
        inner: Arc::downgrade(&state.0),
        ticket: replacement,
    };
    let (started, observing) = oneshot::channel();
    let supervisor = tokio::spawn(async move {
        let _cleanup = cleanup;
        let _sent = started.send(());
        std::future::pending::<()>().await;
    });
    observing.await?;
    supervisor.abort();
    assert!(supervisor.await.is_err_and(|e| e.is_cancelled()));
    assert!(cancelled_receipt.await.is_err());
    assert!(
        state
            .0
            .theme_delivery
            .lock()
            .map_err(|_| "delivery mutex")?
            .is_none()
    );
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 1).is_err());
    pages.record(tauri::webview::PageLoadEvent::Started);
    assert!(advance(&state, peer, "run", &page, generation, &receipt.ticket, 1).is_err());
    assert_eq!(registry.state(peer, id)?.lifecycle, Lifecycle::Running);
    registry.shutdown()?;
    assert_eq!(task.join().await?.lifecycle, Lifecycle::Closed);
    drop((binding, driver, a, b));
    Ok(())
}
