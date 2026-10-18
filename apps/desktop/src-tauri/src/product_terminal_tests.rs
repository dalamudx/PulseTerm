#![allow(clippy::unwrap_used)]
use super::*;
use pulse_app::registry::{PageLaunchScope, SessionId};
use pulse_term::session::{self, ChannelTransport, Lifecycle, SessionHandle, SessionTask};
use tokio::time::timeout;

#[test]
fn alternate_history_search_requires_a_closed_boolean_scope() {
    let args = serde_json::json!({"query":"text","matchCase":false,"regex":false,"wholeWord":false,"backwards":false,"alternate":true,"cursor":null,"revision":null});
    assert!(
        serde_json::from_value::<HistorySearchArgs>(args.clone())
            .unwrap()
            .alternate
    );
    for field in ["alternate", "regex", "wholeWord"] {
        for value in [
            serde_json::Value::Null,
            serde_json::json!("true"),
            serde_json::json!(1),
        ] {
            let mut bad = args.clone();
            bad[field] = value;
            assert!(serde_json::from_value::<HistorySearchArgs>(bad).is_err());
        }
        let mut missing = args.clone();
        missing.as_object_mut().unwrap().remove(field);
        assert!(serde_json::from_value::<HistorySearchArgs>(missing).is_err());
    }
    let mut mixed = args;
    mixed["scope"] = serde_json::json!("primary");
    assert!(serde_json::from_value::<HistorySearchArgs>(mixed).is_err());
}
struct Fixture {
    bank: Arc<TerminalBank>,
    registry: SessionRegistry,
    owner: pulse_app::registry::WindowToken,
    page: Arc<PageLaunchScope>,
    id: SessionId,
    handle: SessionHandle,
    task: SessionTask,
    nonce: Uuid,
    _driver: pulse_term::session::DriverEndpoint,
}
impl Fixture {
    fn new() -> Self {
        let registry = SessionRegistry::new();
        let owner = registry.open_window().unwrap();
        let page = Arc::new(registry.activate_launch_page(owner).unwrap());
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) =
            session::spawn_session(pulse_core::TermSize::new(32, 8).unwrap(), 16, transport);
        let id = registry.register(owner, handle.clone()).unwrap();
        Self {
            bank: Arc::new(TerminalBank::new()),
            registry,
            owner,
            page,
            id,
            handle,
            task,
            nonce: Uuid::new_v4(),
            _driver: driver,
        }
    }
    async fn prepare(&self) -> Prepared {
        self.bank
            .prepare(&self.registry, self.page.clone(), self.id, self.nonce)
            .await
            .unwrap()
    }
    async fn finish(self) {
        timeout(Duration::from_secs(2), self.bank.shutdown())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(self.bank.credit.available_permits(), MAX_VIEWS);
        assert_eq!(self.handle.state.borrow().lifecycle, Lifecycle::Running);
        self.handle.close();
        timeout(Duration::from_secs(2), self.task.join())
            .await
            .unwrap()
            .unwrap();
    }
}
#[tokio::test]
async fn raw_channel_bootstrap_waits_for_explicit_ack_and_connect_is_one_shot() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    assert!(prepared.palette.contains("\"version\":1"));
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    let ticket = ticket(&prepared.ticket).unwrap();
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel(); // Test observation only.
    let channel = tauri::ipc::Channel::new(move |body| {
        tx.send(body).unwrap();
        Ok(())
    });
    f.bank.connect(f.nonce, ticket, channel.clone()).unwrap();
    assert!(f.bank.connect(f.nonce, ticket, channel).is_err());
    loop {
        let body = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        let tauri::ipc::InvokeResponseBody::Raw(wire) = body else {
            panic!("serialized terminal bytes");
        };
        let frame = pulse_ipc::decode(&wire).unwrap();
        let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload).unwrap();
        assert_eq!(fragment.generation.to_string(), prepared.generation);
        f.bank.ack(f.nonce, ticket, frame.sequence).unwrap();
        if fragment.part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
    // The IPC ACK response is queue admission, not synchronous protocol validation.
    f.bank.ack(f.nonce, ticket, u32::MAX).unwrap();
    let all = timeout(
        Duration::from_secs(2),
        f.bank.credit.clone().acquire_many_owned(MAX_VIEWS as u32),
    )
    .await
    .unwrap()
    .unwrap();
    drop(all);
    assert!(f.bank.entry(f.nonce, ticket).is_err());
    assert!(f.bank.ack(f.nonce, ticket, 1).is_err());
    f.finish().await;
}
#[tokio::test]
async fn foreign_nonce_unknown_ticket_and_retired_page_cannot_connect_or_ack() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    let ticket = ticket(&prepared.ticket).unwrap();
    assert!(f.bank.entry(Uuid::new_v4(), ticket).is_err());
    assert!(f.bank.entry(f.nonce, Uuid::new_v4()).is_err());
    let _new_page = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(f.bank.ack(f.nonce, ticket, 0).is_err());
    assert!(
        f.bank
            .connect(f.nonce, ticket, tauri::ipc::Channel::new(|_| Ok(())))
            .is_err()
    );
    f.finish().await;
}
#[tokio::test]
async fn expired_unconnected_capture_releases_credit_only_after_relay_exits() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    let ticket = ticket(&prepared.ticket).unwrap();
    let all = timeout(
        Duration::from_secs(4),
        f.bank.credit.clone().acquire_many_owned(MAX_VIEWS as u32),
    )
    .await
    .unwrap()
    .unwrap();
    drop(all);
    assert!(f.bank.entry(f.nonce, ticket).is_err());
    f.finish().await;
}
#[tokio::test]
async fn channel_send_failure_revokes_view_but_not_actor() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    let ticket = ticket(&prepared.ticket).unwrap();
    let (failed, observed) = oneshot::channel();
    let failed = Mutex::new(Some(failed));
    f.bank
        .connect(
            f.nonce,
            ticket,
            tauri::ipc::Channel::new(move |_| {
                if let Some(tx) = failed.lock().unwrap().take() {
                    let _ = tx.send(());
                }
                Err(std::io::Error::other("fixed sender failure").into())
            }),
        )
        .unwrap();
    timeout(Duration::from_secs(2), observed)
        .await
        .unwrap()
        .unwrap();
    f.finish().await;
}
#[tokio::test]
async fn shutdown_seals_prepare_and_retiring_entry_holds_quota_until_actual_exit() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    let entry = f
        .bank
        .entry(f.nonce, ticket(&prepared.ticket).unwrap())
        .unwrap();
    f.bank.retire();
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    drop(entry);
    f.bank.shutdown().await.unwrap();
    assert!(
        f.bank
            .prepare(&f.registry, f.page.clone(), f.id, f.nonce)
            .await
            .is_err()
    );
    f.finish().await;
}
#[tokio::test]
async fn detach_rejects_late_controls_and_does_not_close_session() {
    let f = Fixture::new();
    let prepared = f.prepare().await;
    let ticket = ticket(&prepared.ticket).unwrap();
    f.bank.detach(f.nonce, ticket).unwrap();
    assert!(f.bank.detach(f.nonce, ticket).is_err());
    assert!(f.bank.ack(f.nonce, ticket, 0).is_err());
    f.finish().await;
}

#[tokio::test]
async fn capture_confirmation_rejects_changed_coordinates_foreign_page_and_duplicate_quota() {
    let f = Fixture::new();
    let captured = f.handle.diagnostic_digest().await.unwrap();
    let prepared = f.prepare().await;
    let ticket = ticket(&prepared.ticket).unwrap();
    let mut rendered = CaptureDrawing {
        columns: captured.boundary.size.columns(),
        rows: captured.boundary.size.rows(),
        revision: captured.boundary.revision.to_string(),
        source_bytes: captured.boundary.parsed_bytes.to_string(),
        utf8_bytes: captured.boundary.utf8_bytes.to_string(),
        digest: captured.digest,
    };
    assert!(
        f.bank
            .confirm(Uuid::new_v4(), ticket, &rendered)
            .await
            .is_err()
    );
    rendered.source_bytes = "00".into();
    assert!(f.bank.confirm(f.nonce, ticket, &rendered).await.is_err());
    rendered.source_bytes = captured.boundary.parsed_bytes.to_string();
    f.bank.confirm(f.nonce, ticket, &rendered).await.unwrap();
    assert!(f.bank.confirm(f.nonce, ticket, &rendered).await.is_err());
    let _new = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(f.bank.confirm(f.nonce, ticket, &rendered).await.is_err());
    f.finish().await;
}
#[test]
fn capture_dto_rejects_unknown_fields_and_lossy_numeric_coordinates() {
    let valid = serde_json::json!({"columns":32,"rows":8,"revision":"0","sourceBytes":"0","utf8Bytes":"0","digest":"0000000000000000"});
    assert!(serde_json::from_value::<CaptureDrawing>(valid.clone()).is_ok());
    let mut bad = valid.clone();
    bad["permission"] = serde_json::json!(true);
    assert!(serde_json::from_value::<CaptureDrawing>(bad).is_err());
    let mut bad = valid;
    bad["sourceBytes"] = serde_json::json!(0);
    assert!(serde_json::from_value::<CaptureDrawing>(bad).is_err());
}

#[tokio::test(flavor = "current_thread")]
async fn resize_bank_waiter_cancellation_keeps_actual_native_work_until_ack() {
    let mut f = Fixture::new();
    let ticket = input_ready(&f).await;
    assert!(f.bank.resize(f.nonce, ticket, 0, 10).await.is_err());
    let mut request = Box::pin(f.bank.resize(f.nonce, ticket, 48, 10));
    let native = tokio::select! {biased;r=&mut request=>panic!("no native ACK {r:?}"),native=f._driver.input.recv()=>native};
    let Some(session::TransportWrite::Resize(size, ack)) = native else {
        panic!("wrong operation")
    };
    assert_eq!(size, pulse_core::TermSize::new(48, 10).unwrap());
    drop(request);
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    assert!(
        f.bank
            .input(f.nonce, ticket, b"old-size".to_vec())
            .await
            .is_err()
    );
    assert!(f.bank.resize(f.nonce, ticket, 60, 12).await.is_err());
    assert!(f.bank.enable_input(f.nonce, ticket).await.is_err()); // Cannot reopen previously validated old-view input during native work.
    ack.send(Ok(())).unwrap();
    let _all = timeout(
        Duration::from_secs(2),
        f.bank.credit.clone().acquire_many_owned(MAX_VIEWS as u32),
    )
    .await
    .unwrap()
    .unwrap();
    assert_eq!(f.handle.state.borrow().size, size);
    drop(_all);
    f.finish().await;
}

#[tokio::test]
async fn layout_before_commit_keeps_input_forbidden_and_retains_io_after_waiter_cancel() {
    let mut f = Fixture::new();
    let p = f.prepare().await;
    let key = ticket(&p.ticket).unwrap();
    assert!(f.bank.resize(f.nonce, key, 48, 10).await.is_err()); // No Channel.
    f.bank
        .connect(f.nonce, key, tauri::ipc::Channel::new(|_| Ok(())))
        .unwrap();
    assert!(f.bank.enable_input(f.nonce, key).await.is_err());
    assert!(f.bank.input(f.nonce, key, b"never".to_vec()).await.is_err());
    let mut request = Box::pin(f.bank.resize(f.nonce, key, 48, 10));
    let op = tokio::select! {biased;r=&mut request=>panic!("no native ACK {r:?}"),op=f._driver.input.recv()=>op};
    let Some(session::TransportWrite::Resize(size, ack)) = op else {
        panic!("not resize")
    };
    let entry = f.bank.entry(f.nonce, key).unwrap();
    assert!(!entry.confirmed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(entry.input_credit.available_permits(), 0);
    drop(request);
    assert_eq!(entry.input_credit.available_permits(), 0);
    assert!(f.bank.enable_input(f.nonce, key).await.is_err());
    assert!(f.bank.input(f.nonce, key, b"never".to_vec()).await.is_err());
    assert!(f.bank.resize(f.nonce, key, 60, 12).await.is_err());
    ack.send(Ok(())).unwrap();
    let credit = timeout(
        Duration::from_secs(2),
        entry.input_credit.clone().acquire_owned(),
    )
    .await
    .unwrap()
    .unwrap();
    drop(credit);
    assert_eq!(f.handle.state.borrow().size, size);
    assert!(f.bank.input(f.nonce, key, b"old".to_vec()).await.is_err());
    drop(entry);
    f.finish().await;
}
#[tokio::test]
async fn layout_shutdown_waiter_cancel_cannot_release_entry_before_native_ack() {
    let mut f = Fixture::new();
    let p = f.prepare().await;
    let key = ticket(&p.ticket).unwrap();
    f.bank
        .connect(f.nonce, key, tauri::ipc::Channel::new(|_| Ok(())))
        .unwrap();
    let mut resize = Box::pin(f.bank.resize(f.nonce, key, 48, 10));
    let op =
        tokio::select! {biased;r=&mut resize=>panic!("early {r:?}"),op=f._driver.input.recv()=>op};
    let Some(session::TransportWrite::Resize(size, ack)) = op else {
        panic!("not resize")
    };
    drop(resize);
    let mut shutdown = Box::pin(f.bank.shutdown());
    tokio::select! {biased;r=&mut shutdown=>panic!("shutdown before native exit {r:?}"),_=tokio::task::yield_now()=>{}}
    drop(shutdown);
    f.registry.close_window(f.owner).unwrap(); // Actual window authority retirement, not just bank cancellation.
    assert!(
        f.bank
            .prepare(&f.registry, f.page.clone(), f.id, f.nonce)
            .await
            .is_err()
    );
    assert!(f.bank.entry(f.nonce, key).is_err());
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    assert_eq!(f.handle.state.borrow().lifecycle, Lifecycle::Running);
    ack.send(Ok(())).unwrap();
    timeout(Duration::from_secs(2), f.bank.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.handle.state.borrow().size, size);
    assert!(f.bank.enable_input(f.nonce, key).await.is_err());
    assert!(f.bank.input(f.nonce, key, b"late".to_vec()).await.is_err());
    f.finish().await;
}
#[tokio::test]
async fn layout_native_error_never_publishes_size_or_reopens_input_after_retirement() {
    let mut f = Fixture::new();
    let old = f.handle.state.borrow().size;
    let p = f.prepare().await;
    let key = ticket(&p.ticket).unwrap();
    f.bank
        .connect(f.nonce, key, tauri::ipc::Channel::new(|_| Ok(())))
        .unwrap();
    let mut resize = Box::pin(f.bank.resize(f.nonce, key, 48, 10));
    let op =
        tokio::select! {biased;r=&mut resize=>panic!("early {r:?}"),op=f._driver.input.recv()=>op};
    let Some(session::TransportWrite::Resize(_, ack)) = op else {
        panic!("not resize")
    };
    f.bank.retire();
    assert!(f.bank.entry(f.nonce, key).is_err());
    ack.send(Err(session::SessionError::Transport)).unwrap();
    assert!(resize.await.is_err());
    assert_eq!(f.handle.state.borrow().size, old);
    assert!(f.bank.resize(f.nonce, key, 60, 12).await.is_err());
    assert!(f.bank.enable_input(f.nonce, key).await.is_err());
    f.finish().await;
}
#[tokio::test]
async fn layout_before_commit_rejects_retired_page_and_replaced_generation() {
    let f = Fixture::new();
    let p = f.prepare().await;
    let old = ticket(&p.ticket).unwrap();
    f.bank
        .connect(f.nonce, old, tauri::ipc::Channel::new(|_| Ok(())))
        .unwrap();
    let fresh = f.prepare().await;
    assert!(f.bank.resize(f.nonce, old, 48, 10).await.is_err());
    let key = ticket(&fresh.ticket).unwrap();
    f.bank
        .connect(f.nonce, key, tauri::ipc::Channel::new(|_| Ok(())))
        .unwrap();
    let _new = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(f.bank.resize(f.nonce, key, 48, 10).await.is_err());
    f.finish().await;
}
#[tokio::test]
async fn completed_actor_new_page_draws_frozen_capture_but_never_enables_input_or_resize() {
    let Fixture {
        bank,
        registry,
        owner,
        page,
        id,
        handle,
        task,
        nonce,
        _driver,
    } = Fixture::new();
    _driver
        .output
        .send(b"final frozen image\r\n".to_vec())
        .await
        .unwrap();
    drop(_driver.output);
    let final_state = task.join_with_receipt().await.unwrap().state;
    assert_eq!(final_state.lifecycle, Lifecycle::Ended);
    let new_page = Arc::new(registry.activate_launch_page(owner).unwrap());
    assert!(bank.prepare(&registry, page, id, nonce).await.is_err());
    let p = bank
        .prepare(&registry, new_page.clone(), id, nonce)
        .await
        .unwrap();
    assert!(p.ended);
    let key = ticket(&p.ticket).unwrap();
    let entry = bank.entry(nonce, key).unwrap();
    assert_eq!(entry.capture, final_state);
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    bank.connect(
        nonce,
        key,
        tauri::ipc::Channel::new(move |body| {
            tx.send(body).unwrap();
            Ok(())
        }),
    )
    .unwrap();
    let snapshot = handle
        .diagnostic_subscribe(
            2000,
            pulse_term::session::MAX_HISTORY_BYTES,
            pulse_term::session::MAX_RESTORE_BYTES,
        )
        .await
        .unwrap();
    let digest = snapshot.bootstrap.digest;
    bank.confirm(
        nonce,
        key,
        &CaptureDrawing {
            columns: final_state.size.columns(),
            rows: final_state.size.rows(),
            revision: final_state.revision.to_string(),
            source_bytes: final_state.parsed_bytes.to_string(),
            utf8_bytes: final_state.utf8_bytes.to_string(),
            digest,
        },
    )
    .await
    .unwrap();
    assert!(bank.enable_input(nonce, key).await.is_err());
    assert!(bank.input(nonce, key, b"never".to_vec()).await.is_err());
    assert!(bank.resize(nonce, key, 48, 10).await.is_err());
    loop {
        let tauri::ipc::InvokeResponseBody::Raw(wire) = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap()
        else {
            panic!("non raw")
        };
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach_v2::decode_fragment(frame.payload)
            .unwrap()
            .part;
        bank.ack(nonce, key, frame.sequence).unwrap();
        if part == pulse_ipc::attach::Part::End {
            break;
        }
    }
    drop(entry);
    timeout(Duration::from_secs(2), bank.shutdown())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(handle.state.borrow().lifecycle, Lifecycle::Ended);
    let _ = new_page;
}
async fn input_ready(f: &Fixture) -> Uuid {
    let captured = f.handle.diagnostic_digest().await.unwrap();
    let p = f.prepare().await;
    let ticket = ticket(&p.ticket).unwrap();
    assert!(
        f.bank
            .input(f.nonce, ticket, b"before".to_vec())
            .await
            .is_err()
    );
    assert!(f.bank.enable_input(f.nonce, ticket).await.is_err());
    f.bank
        .confirm(
            f.nonce,
            ticket,
            &CaptureDrawing {
                columns: captured.boundary.size.columns(),
                rows: captured.boundary.size.rows(),
                revision: captured.boundary.revision.to_string(),
                source_bytes: captured.boundary.parsed_bytes.to_string(),
                utf8_bytes: captured.boundary.utf8_bytes.to_string(),
                digest: captured.digest,
            },
        )
        .await
        .unwrap();
    assert!(f.bank.enable_input(f.nonce, ticket).await.is_err()); // no Channel connected
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
    f.bank
        .connect(
            f.nonce,
            ticket,
            tauri::ipc::Channel::new(move |body| {
                tx.send(body).unwrap();
                Ok(())
            }),
        )
        .unwrap();
    assert!(f.bank.enable_input(f.nonce, ticket).await.is_err()); // Commit emitted != validated ACK
    loop {
        let tauri::ipc::InvokeResponseBody::Raw(wire) = rx.recv().await.unwrap() else {
            panic!("non-raw")
        };
        let frame = pulse_ipc::decode(&wire).unwrap();
        let part = pulse_ipc::attach_v2::decode_fragment(frame.payload)
            .unwrap()
            .part;
        f.bank.ack(f.nonce, ticket, frame.sequence).unwrap();
        if part == pulse_ipc::attach::Part::Commit {
            break;
        }
    }
    f.bank.enable_input(f.nonce, ticket).await.unwrap();
    ticket
}
#[tokio::test]
async fn input_gate_exact_utf8_requires_confirmed_current_source_and_validated_commit() {
    let mut f = Fixture::new();
    let ticket = input_ready(&f).await;
    let bytes = "输入🙂\r".as_bytes().to_vec();
    f.bank.input(f.nonce, ticket, bytes.clone()).await.unwrap();
    let Some(session::TransportWrite::Input(actual)) = f._driver.input.recv().await else {
        panic!("wrong write")
    };
    assert_eq!(actual, bytes);
    for invalid in [vec![], vec![b'a'; 1025], vec![0xff]] {
        assert!(f.bank.input(f.nonce, ticket, invalid).await.is_err());
    }
    assert!(
        f.bank
            .input(Uuid::new_v4(), ticket, b"foreign".to_vec())
            .await
            .is_err()
    );
    let _new = f.registry.activate_launch_page(f.owner).unwrap();
    assert!(
        f.bank
            .input(f.nonce, ticket, b"old".to_vec())
            .await
            .is_err()
    );
    assert!(f._driver.input.try_recv().is_err());
    f.finish().await;
}
#[tokio::test(flavor = "current_thread")]
async fn input_gate_waiter_cancellation_retains_permit_and_busy_never_retries() {
    let f = Fixture::new();
    let ticket = input_ready(&f).await;
    for _ in 0..f._driver.input.max_capacity() {
        f.bank.input(f.nonce, ticket, b"q".to_vec()).await.unwrap();
    }
    let mut pending = Box::pin(f.bank.input(f.nonce, ticket, b"blocked".to_vec()));
    tokio::select! {biased;_=&mut pending=>panic!("queue unexpectedly free"),_=std::future::ready(())=>{}}
    drop(pending);
    assert!(matches!(
        f.bank.input(f.nonce, ticket, b"duplicate".to_vec()).await,
        Err("workspace.busy")
    ));
    assert_eq!(f.bank.credit.available_permits(), MAX_VIEWS - 1);
    f.bank.retire();
    f.finish().await;
}

#[test]
fn malformed_or_noncanonical_ticket_is_rejected() {
    assert!(ticket("not-a-uuid").is_err());
    assert!(ticket(&Uuid::new_v4().simple().to_string()).is_err());
}

#[path = "product_terminal_clipboard_tests.rs"]
mod clipboard_tests;

#[path = "product_terminal_completed_tests.rs"]
mod completed_tests;
