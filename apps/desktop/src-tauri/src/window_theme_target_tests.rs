use super::*;
use pulse_term::session::{ChannelTransport, spawn_session};
use std::os::unix::fs::PermissionsExt;
#[tokio::test]
async fn real_migrated_target_requires_draw_source_claim_epoch_and_single_use()
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
    let previous = registry
        .attach(peer, id, AttachmentConfig::default())
        .await?;
    registry.transfer(peer, main, id)?;
    registry.close_window(peer)?;
    let binding = registry
        .attach(main, id, AttachmentConfig::default())
        .await?;
    let generation = binding.generation;
    assert!(generation > previous.generation);
    let mut run = Active::new(
        main,
        "run".into(),
        id,
        Arc::new(crate::viewer_visibility::VisibilityGate::default()),
    );
    run.peer_owner = Some(peer);
    run.peer_id = Some(id);
    *state.0.active.lock().map_err(|_| "test mutex")? = Some(run);
    registry.configure_view_theme(main, id, generation, a.handle())?;
    let candidate = registry
        .begin_view_theme(main, id, generation)?
        .ready()
        .await?;
    let snapshot = registry.confirm_view_theme(main, id, generation, candidate)?;
    let ticket = uuid::Uuid::new_v4();
    let claim = ticket.to_string();
    let (done, mut receipt) = oneshot::channel();
    *state.0.theme_delivery.lock().map_err(|_| "test mutex")? = Some(Challenge {
        owner: main,
        id,
        generation,
        page: claim.clone(),
        target_epoch: Some(0),
        ticket,
        snapshot: snapshot.clone(),
        phase: 0,
        done: Some(done),
        retirement: None,
    });
    assert!(read(&state, main, "run", generation, &claim).is_err()); // No target capture.
    let lost = transfer_view::theme_test_capture(&state, main, id, generation, true, false)?;
    drop(lost);
    assert!(transfer_view::confirm_for(&state, main, id, generation).is_err());
    assert!(read(&state, main, "run", generation, &claim).is_err()); // Lost draw receipt cannot arm target.
    for (acknowledged, live) in [(false, false), (true, true)] {
        let (drawn, _ready) =
            transfer_view::theme_test_capture(&state, main, id, generation, acknowledged, live)?;
        transfer_view::confirm_for(&state, main, id, generation)?;
        drawn.await?;
        assert!(read(&state, main, "run", generation, &claim).is_err());
    }
    let (drawn, ready) =
        transfer_view::theme_test_capture(&state, main, id, generation, true, false)?;
    // Palette is required before replay, not only after the draw handshake.
    let palette = transfer_view::captured_palette(&state, main, "run", generation)?
        .ok_or("missing palette")?;
    let palette: serde_json::Value = serde_json::from_str(&palette)?;
    assert_eq!(palette["version"], 1);
    assert_eq!(
        palette["palette"]["ansi"]
            .as_array()
            .ok_or("missing ANSI")?
            .len(),
        16
    );
    assert!(transfer_view::captured_palette(&state, foreign, "run", generation).is_err());
    assert!(transfer_view::captured_palette(&state, peer, "run", generation).is_err());
    assert!(transfer_view::captured_palette(&state, main, "wrong", generation).is_err());
    assert!(transfer_view::captured_palette(&state, main, "run", generation + 1).is_err());
    assert!(transfer_view::captured_palette(&state, main, "run", previous.generation).is_err());
    assert!(failed(&state, main, "run", generation, &claim, 6).is_err());
    assert!(failed(&state, foreign, "run", generation, &claim, 0).is_err());
    assert!(failed(&state, main, "wrong", generation, &claim, 0).is_err());
    assert!(failed(&state, main, "run", generation + 1, &claim, 0).is_err());
    assert!(read(&state, main, "run", generation, &claim).is_err()); // Before actual summary confirmation.
    transfer_view::confirm_for(&state, main, id, generation)?;
    drawn.await?;
    assert!(read(&state, main, "run", generation, &claim).is_err());
    transfer_view::ready_theme_for(&state, main, id, generation)?;
    ready.await?;
    assert!(transfer_view::ready_theme_for(&state, main, id, generation).is_err());
    for (owner, nonce, g, token) in [
        (peer, "run", generation, claim.as_str()),
        (foreign, "run", generation, claim.as_str()),
        (main, "wrong", generation, claim.as_str()),
        (main, "run", generation + 1, claim.as_str()),
        (main, "run", generation, "wrong"),
    ] {
        assert!(read(&state, owner, nonce, g, token).is_err());
    }
    assert!(advance(&state, main, "run", generation, &claim, 2).is_err());
    let delivered = read(&state, main, "run", generation, &claim)?;
    assert_eq!(delivered.ticket, claim);
    assert!(delivered.paint_json.len() <= 4096);
    assert!(read(&state, main, "run", generation, &claim).is_err());
    advance(&state, main, "run", generation, &claim, 1)?;
    assert!(advance(&state, main, "run", generation, &claim, 1).is_err());
    assert!(cleaned(&state, main, "run", generation, &claim).is_err());
    for (owner, nonce, g, token) in [
        (peer, "run", generation, claim.as_str()),
        (foreign, "run", generation, claim.as_str()),
        (main, "wrong", generation, claim.as_str()),
        (main, "run", generation + 1, claim.as_str()),
        (main, "run", generation, "wrong"),
    ] {
        assert!(wait_revoked(&state, owner, nonce, g, token).await.is_err());
    }
    use std::future::Future;
    let mut notice = Box::pin(wait_revoked(&state, main, "run", generation, &claim));
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(notice.as_mut().poll(&mut cx).is_pending());
    receipt.try_recv()?;
    assert!(
        wait_revoked(&state, main, "run", generation, &claim)
            .await
            .is_err()
    );
    assert!(cleaned(&state, main, "run", generation, &claim).is_err());
    let (done, mut receipt) = oneshot::channel();
    state
        .0
        .theme_delivery
        .lock()
        .map_err(|_| "test mutex")?
        .as_mut()
        .ok_or("test slot")?
        .done = Some(done);
    registry.invalidate_view_theme(main, id, generation)?;
    notice.await?;
    cleaned(&state, main, "run", generation, &claim)?;
    receipt.try_recv()?;
    assert!(cleaned(&state, main, "run", generation, &claim).is_err());
    {
        let mut slot = state.0.theme_delivery.lock().map_err(|_| "test mutex")?;
        let c = slot.as_mut().ok_or("test challenge missing")?;
        c.phase = 0;
        c.target_epoch = None;
    }
    assert!(read(&state, main, "run", generation, &claim).is_err()); // Peer challenge is not a main capability.
    {
        let mut slot = state.0.theme_delivery.lock().map_err(|_| "test mutex")?;
        slot.as_mut().ok_or("test challenge missing")?.target_epoch = Some(0);
    }
    registry.configure_view_theme(main, id, generation, b.handle())?;
    registry.configure_view_theme(main, id, generation, a.handle())?;
    assert!(read(&state, main, "run", generation, &claim).is_err()); // ABA cannot revive captured source.
    let candidate = registry
        .begin_view_theme(main, id, generation)?
        .ready()
        .await?;
    let fresh = registry.confirm_view_theme(main, id, generation, candidate)?;
    {
        let mut slot = state.0.theme_delivery.lock().map_err(|_| "test mutex")?;
        slot.as_mut().ok_or("test challenge missing")?.snapshot = fresh;
    }
    read(&state, main, "run", generation, &claim)?;
    advance(&state, main, "run", generation, &claim, 1)?;
    let (done, mut ready) = oneshot::channel();
    state
        .0
        .theme_delivery
        .lock()
        .map_err(|_| "test slot")?
        .as_mut()
        .ok_or("slot")?
        .done = Some(done);
    let mut pending = Box::pin(wait_revoked(&state, main, "run", generation, &claim));
    assert!(pending.as_mut().poll(&mut cx).is_pending());
    ready.try_recv()?;
    retire_main_document(&state);
    assert!(
        tokio::time::timeout(Duration::from_secs(1), pending)
            .await?
            .is_err()
    );
    assert!(
        registry
            .read_current_view_theme(main, id, generation)?
            .is_some()
    ); // Document retirement does not close actor or theme source.
    assert!(
        state
            .0
            .theme_delivery
            .lock()
            .map_err(|_| "test mutex")?
            .is_none()
    );
    assert!(advance(&state, main, "run", generation, &claim, 1).is_err());
    assert_eq!(registry.state(main, id)?.lifecycle, Lifecycle::Running);
    registry.check_binding(main, id, generation)?;
    // Saturating exhaustion is fail closed, never wraps to a prior epoch.
    state
        .0
        .theme_target_epoch
        .store(u64::MAX - 1, Ordering::SeqCst);
    retire_main_document(&state);
    retire_main_document(&state);
    assert_eq!(state.0.theme_target_epoch.load(Ordering::SeqCst), u64::MAX);
    *state.0.theme_delivery.lock().map_err(|_| "test mutex")? = Some(Challenge {
        owner: main,
        id,
        generation,
        page: claim.clone(),
        target_epoch: Some(u64::MAX),
        ticket,
        snapshot,
        phase: 0,
        done: None,
        retirement: None,
    });
    // Reach the epoch check with a genuinely ready target; not_ready cannot mask exhaustion.
    let (drawn, ready) =
        transfer_view::theme_test_capture(&state, main, id, generation, true, false)?;
    transfer_view::confirm_for(&state, main, id, generation)?;
    drawn.await?;
    transfer_view::ready_theme_for(&state, main, id, generation)?;
    ready.await?;
    state
        .0
        .theme_target_epoch
        .store(u64::MAX - 1, Ordering::SeqCst);
    {
        let mut slot = state.0.theme_delivery.lock().map_err(|_| "test mutex")?;
        slot.as_mut().ok_or("test challenge missing")?.target_epoch = Some(u64::MAX - 1);
    }
    authorized(&state, main, "run", generation, &claim, |_| Ok(()))?;
    state.0.theme_target_epoch.store(u64::MAX, Ordering::SeqCst);
    {
        let mut slot = state.0.theme_delivery.lock().map_err(|_| "test mutex")?;
        slot.as_mut().ok_or("test challenge missing")?.target_epoch = Some(u64::MAX);
    }
    assert!(authorized(&state, main, "run", generation, &claim, |_| Ok(())).is_err());
    state
        .0
        .theme_delivery
        .lock()
        .map_err(|_| "test mutex")?
        .take();
    registry.close_session(main, id)?;
    tokio::time::timeout(Duration::from_secs(1), task.join()).await??;
    a.close();
    b.close();
    drop((binding, previous, driver));
    Ok(())
}
