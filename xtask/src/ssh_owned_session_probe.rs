//! Real loopback SSH to root-owned actor, synthetic private credentials only; no product entry.
use super::*;
#[path = "ssh_workspace_fixture.rs"]
pub(crate) mod gui;
use anyhow::Context as _;
use pulse_app::protocol_session_owner::ProtocolSessionReservation;
use pulse_term::session::Lifecycle;
use russh::{Channel, ChannelId, Pty};
struct SessionSpy {
    auth: AcceptSpy,
    pty: bool,
    resized: tokio::sync::watch::Sender<Option<(u32, u32)>>,
    palette_replies: tokio::sync::watch::Sender<u8>,
    reply_buffer: Vec<u8>,
}
impl Server for SessionSpy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            auth: AcceptSpy {
                auth: self.auth.auth.clone(),
                exact: self.auth.exact.clone(),
            },
            pty: false,
            resized: self.resized.clone(),
            palette_replies: self.palette_replies.clone(),
            reply_buffer: Vec::new(),
        }
    }
}
impl server::Handler for SessionSpy {
    type Error = russh::Error;
    async fn auth_none(&mut self, user: &str) -> Result<server::Auth, Self::Error> {
        server::Handler::auth_none(&mut self.auth, user).await
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        server::Handler::auth_password(&mut self.auth, user, password).await
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(Pty, u32)],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        self.pty = term == "xterm-256color" && cols == 32 && rows == 8;
        if self.pty {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn window_change_request(
        &mut self,
        _: ChannelId,
        columns: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        // Observation only, intentionally NO channel_success/remote ACK (RFC4254).
        self.resized.send_replace(Some((columns, rows)));
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if !self.pty {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        session.data(channel, b"\x1b]11;?\x07ROOT_READY\r\n".to_vec())?;
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if data.len() > 128 || !self.pty {
            return Err(russh::Error::Disconnect);
        }
        // Collect the fixed OSC reply across SSH data frames; never echo a query reply.
        if data.starts_with(b"\x1b") || !self.reply_buffer.is_empty() {
            self.reply_buffer.extend_from_slice(data);
            if self.reply_buffer.len() > 128 {
                return Err(russh::Error::Disconnect);
            }
            if self.reply_buffer.last() != Some(&7) {
                return Ok(());
            }
            let bit = match self.reply_buffer.as_slice() {
                b"\x1b]11;rgb:fafa/fafa/f9f9\x07" => 1,
                b"\x1b]11;rgb:0f0f/1717/2a2a\x07" => 2,
                _ => return Err(russh::Error::Disconnect),
            };
            self.reply_buffer.clear();
            self.palette_replies.send_modify(|value| *value |= bit);
            return Ok(());
        }
        if data == b"exit" {
            session.data(channel, b"ROOT_END\r\n".to_vec())?;
            session.exit_status_request(channel, 7)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            let mut echo = b"ROOT_ECHO:".to_vec();
            echo.extend_from_slice(data);
            echo.extend_from_slice(b"\r\n");
            session.data(channel, echo)?;
        }
        Ok(())
    }
}
pub(crate) fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(4).build()?.block_on(async {
        let registry=SessionRegistry::new();let documents=ProductDocuments::new(registry.clone());
        let window=registry.open_window()?;documents.install(window)?;documents.started(window)?;
        let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;
        let page=documents.current(window,nonce)?;let owner=PageVaultOwner::new(registry.clone());
        let root=tempfile::tempdir()?;let dir=pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let(store,worker)=StateStore::open(dir.join("state.db")).await?;
        let empty=store.load_vault_ciphertext(Arc::new(||true)).await?;let op=owner.begin(page.clone()).context("stage.create_admit")?;
        let(vault,request)=op.begin_creation(PasswordInput::new(b"fixed-target-master".to_vec())?,empty.clone())?;
        op.persist_creation(&store,empty,vault,request.ready().await?).await?;
        let saved=store.load_vault_ciphertext(Arc::new(||true)).await?;
        let request=op.begin_unlock(PasswordInput::new(b"fixed-target-master".to_vec())?,saved.clone())?;
        op.confirm_unlock(&store,saved,request.ready().await?).await?.accept()?;
        let identities=store.edit_ssh_identities(store.load_ssh_identities(Arc::new(||true)).await?,
            IdentityEdit::Create(SshPasswordIdentity::new("fixed","user",SecretRef::new(vault,Uuid::now_v7(),SecretKind::SshPassword,false)?)?),Arc::new(||true)).await?;
        let identity=identities.records().next().ok_or_else(||anyhow::anyhow!("fixture.identity"))?.id();
        owner.begin(page.clone()).context("stage.password_write_admit")?.prepare_password_identity_write(store.load_identity_vault_source(Arc::new(||true)).await?,identity)?
            .write(&store,SecretBytes::from_owned(b"fixed-target-secret".to_vec())?).await?;
        let key=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7;32]).into(),"")?;
        let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await?;
        let address=listener.local_addr()?;let target=HostTarget::new("127.0.0.1",address.port())?;
        let auth=Arc::new(AtomicUsize::new(0));let exact=Arc::new(std::sync::atomic::AtomicBool::new(false));
        let (resized,mut remote_size)=tokio::sync::watch::channel(None);
        let (palette_replies,mut palette_seen)=tokio::sync::watch::channel(0u8);
        let spy=SessionSpy{auth:AcceptSpy{auth:auth.clone(),exact:exact.clone()},pty:false,resized,palette_replies,reply_buffer:Vec::new()};
        let config=Arc::new(server::Config{keys:vec![key.clone()],..Default::default()});
        let(tx,rx)=tokio::sync::oneshot::channel();let server_task=tokio::spawn(async move{
            let mut spy=spy;let running=spy.run_on_socket(config,&listener);let _=tx.send(running.handle());running.await
        });
        let mut server=ServerGuard{stop:rx.await?,task:Some(server_task)};
        let hosts=store.edit_ssh_hosts(store.load_ssh_hosts().await?,
            HostEdit::Create(SshEndpoint::new("fixed","127.0.0.1",address.port(),"user")?),Arc::new(||true)).await?;
        let host=hosts.records()[0].id();let _=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,key.public_key())?).await?;
        let source=store.load_ssh_target_source(Arc::new(||true)).await?;
        let started=ProtocolSessionReservation::reserve(&registry,&page)?.begin()?;
        let connection=PinnedTransport::connect_stream(target,Some(validated_pins(source.trust())?),None,
            Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await?;
        let peer=connection.peer()?;let native=connection.native_io_completion();
        let guard=peer_guard(&source,host,&connection)?;
        let palette=pulse_app::terminal_palette::query_palette_from_preset(pulse_store::settings::TerminalPreset::Paper);
        let mut adapter=owned_adapter::SessionSink::new(connection,(host,identity),pulse_core::TermSize::new(32,8)?,128)?;
        adapter.set_palette(palette.clone());
        let admission=adapter.observe_admission();
        let intent=owner.begin_password_session_action_authorized(page.clone(),host,identity,guard)?.prepare(&store,source).await?;
        let session=intent.start(store.clone(),adapter,started).wait().await?;
        ensure!(auth.load(Ordering::SeqCst)==1&&exact.load(Ordering::Acquire),"fixture.auth");
        drop(owner.begin(page.clone()).context("stage.first_auth_admission_release")?);
        let handle=session.handle().clone();let mut state=handle.state.clone();
        let mut sessions=pulse_app::local_session_root::SessionRoot::new(registry.clone(),tokio::runtime::Handle::current());
        let id=sessions.adopt_protocol(session)?;
        ensure!(sessions.is_protocol(id)&&sessions.occupied()==1,"fixture.root_adoption");
        tokio::time::timeout(std::time::Duration::from_secs(2),state.wait_for(|s|s.parsed_bytes>=12)).await??;
        tokio::time::timeout(std::time::Duration::from_secs(2),palette_seen.wait_for(|value| *value==1)).await??;
        let first=handle.diagnostic_bootstrap(0,1024,8192).await?;
        ensure!(first.restore.windows(b"ROOT_READY".len()).any(|w|w==b"ROOT_READY"),"fixture.ready_actor");
        ensure!(first.palette==palette,"fixture.paper_capture_base");
        let view=pulse_app::terminal_view::TerminalViewBinding::attach_v2(&registry,page.clone(),id,pulse_app::attachment::AttachmentConfig::default()).await?;
        let delivery=pulse_app::terminal_view::TerminalViewDelivery::new(view)?;
        let view=delivery.binding();
        let requested=pulse_core::TermSize::new(64,16)?;
        let layout=tokio::time::timeout(std::time::Duration::from_secs(2),view.resize_with_receipt(requested)).await??;
        ensure!(layout.size==requested&&layout.basis==Some(pulse_term::session::ResizeBasis::SshWindowRequestAdmitted),"fixture.ssh_layout_basis");
        ensure!(handle.state.borrow().size==requested,"fixture.actor_layout_not_committed");
        tokio::time::timeout(std::time::Duration::from_secs(2),remote_size.wait_for(|size|*size==Some((64,16)))).await??;
        ensure!(view.input(b"forbidden-old-view".to_vec()).await.is_err(),"fixture.resize_enabled_old_input");
        drop(view);drop(delivery);
        documents.started(window)?;owner.retire_page()?;
        ensure!(peer.is_current()&&admission.upgrade().is_some()&&sessions.ids()==vec![id],"fixture.navigation_closed_root");
        let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;
        let current=documents.current(window,nonce)?;registry.check_launch_page(&current)?;
        let before=state.borrow().parsed_bytes;
        handle.input(b"phase-b".to_vec()).await?;
        tokio::time::timeout(std::time::Duration::from_secs(2),state.wait_for(|s|s.parsed_bytes>=before+19)).await??;
        let restored=handle.diagnostic_bootstrap(0,1024,8192).await?;
        ensure!(restored.restore.windows(b"ROOT_ECHO:phase-b".len()).any(|w|w==b"ROOT_ECHO:phase-b"),"fixture.echo_actor");
        ensure!(restored.palette==palette,"fixture.navigation_changed_base");
        handle.input(b"exit".to_vec()).await?;
        tokio::time::timeout(std::time::Duration::from_secs(2),state.wait_for(|s|s.lifecycle==Lifecycle::Ended)).await??;
        tokio::time::timeout(std::time::Duration::from_secs(2),sessions.take_protocol_for_finish(id)?.finish()).await??;
        ensure!(sessions.collect_reaped()==1&&sessions.occupied()==0,"fixture.root_eof_reaped");
        native.wait().await?;
        ensure!(!peer.is_current()&&admission.upgrade().is_none(),"fixture.native_not_joined");
        // Page retirement preserves an accepted Vault key. Explicitly lock before testing
        // a fresh unlock; re-unlocking an already-available epoch is correctly Busy.
        owner.lock().context("stage.explicit_lock_before_second_unlock")?;
        // A second real authenticated connection is cancelled while its native reader waits.
        let encrypted=store.load_vault_ciphertext(Arc::new(||true)).await?;
        let op=owner.begin(current.clone()).context("stage.second_unlock_admit")?;
        let request=op.begin_unlock(PasswordInput::new(b"fixed-target-master".to_vec())?,encrypted.clone()).context("stage.second_unlock_request")?;
        op.confirm_unlock(&store,encrypted,request.ready().await?).await?.accept()?;
        let sessions=Arc::new(std::sync::Mutex::new(sessions));
        let requests=owned_adapter::SessionRequests::new(registry.clone(),window,tokio::runtime::Handle::current());
        // Exercise the production original-epoch boundary, not the test-only unpinned admission.
        let gesture=||owner.begin_password_session_gesture_authorized(current.clone(),host,identity,Arc::new(||true)).map_err(anyhow::Error::msg);
        let ticket=requests.begin_gesture(current.clone(),host,identity,&owner,gesture()?).map_err(anyhow::Error::msg)?;
        let authorization=requests.authorization(&current,ticket).map_err(anyhow::Error::msg)?;
        let read_store=store.clone();
        requests.prepare(current.clone(),ticket,async move{let source=read_store.load_ssh_target_source(authorization).await.map_err(|_|"fixture.source_failed")?;Ok((read_store,source))}).await.map_err(anyhow::Error::msg)?;
        ensure!(requests.authorization(&current,ticket).is_err(),"fixture.repeat_prepare_not_rejected");
        let context=owned_adapter::StartTarget{root:sessions.clone(),size:pulse_core::TermSize::new(32,8)?,history_rows:128,palette:pulse_app::terminal_palette::query_palette_from_preset(pulse_store::settings::TerminalPreset::Midnight).into()};
        let id=requests.start(current.clone(),ticket,context).await.map_err(anyhow::Error::msg)?;
        let mut state=sessions.lock().map_err(|_|anyhow::anyhow!("fixture.root_lock"))?.protocol_handle(id)?.state.clone();
        tokio::time::timeout(std::time::Duration::from_secs(2),state.wait_for(|s|s.parsed_bytes>=12)).await??;
        tokio::time::timeout(std::time::Duration::from_secs(2),palette_seen.wait_for(|value| *value==3)).await??;
        let second=sessions.lock().map_err(|_|anyhow::anyhow!("fixture.root_lock"))?.protocol_handle(id)?.clone();
        ensure!(second.diagnostic_bootstrap(0,1024,8192).await?.palette==pulse_app::terminal_palette::query_palette_from_preset(pulse_store::settings::TerminalPreset::Midnight),"fixture.midnight_capture_base");
        let close=sessions.lock().map_err(|_|anyhow::anyhow!("fixture.root_lock"))?.close_protocol(id)?;
        tokio::time::timeout(std::time::Duration::from_secs(2),close.join()).await??;
        {
            let mut root=sessions.lock().map_err(|_|anyhow::anyhow!("fixture.root_lock"))?;
            ensure!(root.collect_reaped()==1&&root.occupied()==0,"fixture.root_cancel_reaped");
        }
        ensure!(auth.load(Ordering::SeqCst)==2,"fixture.cancel_auth_count");
        ensure!(requests.authorization(&current,ticket).is_err(),"fixture.reused_start_ticket");
        let cancelled=requests.begin_gesture(current.clone(),host,identity,&owner,gesture()?).map_err(anyhow::Error::msg)?;
        requests.cancel(&current,cancelled).map_err(anyhow::Error::msg)?;
        ensure!(requests.authorization(&current,cancelled).is_err(),"fixture.cancelled_ticket_upgraded");
        let retired=requests.begin_gesture(current.clone(),host,identity,&owner,gesture()?).map_err(anyhow::Error::msg)?;
        requests.retire();ensure!(requests.authorization(&current,retired).is_err(),"fixture.retired_ticket");
        requests.shutdown().await.map_err(anyhow::Error::msg)?;
        ensure!(requests.begin_gesture(current.clone(),host,identity,&owner,gesture()?).is_err(),"fixture.closed_requests");
        // Deterministic request-supervisor lifetime: cancel the IPC waiter while the
        // backend read future remains pending. No sleeps, network or fake success receipt.
        let pending_bank=owned_adapter::SessionRequests::new(registry.clone(),window,tokio::runtime::Handle::current());
        // Capture the competing gesture before the old read owns native credit. Capturing it
        // after cancellation correctly returns owner Busy, but would not test the bank's quota.
        let blocked_gesture=gesture()?;
        let pending_ticket=pending_bank.begin_gesture(current.clone(),host,identity,&owner,gesture()?).map_err(anyhow::Error::msg)?;
        let (entered_tx,entered_rx)=tokio::sync::oneshot::channel();
        let (release_tx,release_rx)=tokio::sync::oneshot::channel();
        let read_bank=pending_bank.clone();let read_page=current.clone();
        let reader=tokio::spawn(async move {read_bank.prepare(read_page,pending_ticket,async move {
            let _sent=entered_tx.send(());
            release_rx.await.map_err(|_|"fixture.release_lost")?;
            Err("fixture.cancelled_read_finished")
        }).await});
        tokio::time::timeout(std::time::Duration::from_secs(2),entered_rx).await??;
        reader.abort();ensure!(reader.await.is_err(),"fixture.waiter_not_cancelled");
        ensure!(matches!(pending_bank.begin_gesture(current.clone(),host,identity,&owner,blocked_gesture),Err("workspace.busy")),"fixture.read_credit_released_early");
        let mut drained=Box::pin(pending_bank.shutdown());
        let poll=std::future::poll_fn(|cx|std::task::Poll::Ready(std::future::Future::poll(drained.as_mut(),cx))).await;
        ensure!(poll.is_pending(),"fixture.shutdown_ignored_pending_read");
        release_tx.send(()).map_err(|_|anyhow::anyhow!("fixture.release_failed"))?;
        tokio::time::timeout(std::time::Duration::from_secs(2),drained).await?.map_err(anyhow::Error::msg)?;
        ensure!(auth.load(Ordering::SeqCst)==2,"fixture.cancelled_read_authenticated");
        owner.close()?;drop(store);worker.join().await?;let drain=sessions.lock().map_err(|_|anyhow::anyhow!("fixture.root_lock"))?.shutdown();drain.join().await?;registry.shutdown()?;
        server.stop.shutdown("fixed end".into());if let Some(task)=server.task.take(){tokio::time::timeout(std::time::Duration::from_secs(2),task).await???;}
        let report=crate::root()?.join("target/probes/ssh-owned-session.json");
        std::fs::write(report,serde_json::to_vec_pretty(&serde_json::json!({
            "schemaVersion":1,"productionStarterSourceChecked":true,"singleUseSessionRequestsChecked":true,"cancelledReadRequestCreditHeldUntilExit":true,"authenticationRequests":2,"ciphertextAuthenticated":true,"explicitSessionRole":true,"sameSharedRootAdopted":true,
            "actorRenderedFixedOutput":true,"rootSurvivedPageNavigation":true,"inputEchoAfterNavigation":true,
            "initialOscBackgroundUsesPreset":true,"paperAndMidnightBaseCaptured":true,"presetSurvivesNavigation":true,
            "orderedEof":true,"sdkAndNativeJoined":true,"cancelWhileNativeReadPending":true,"plaintextReturned":false,"productSshOpened":false,
            "sshWindowRequestAdmitted":true,"serverReceivedWindowRequest":true,"actorLayoutAfterRequest":true,"remoteKernelResizeConfirmed":false,"realWebViewChecked":false
        }))?)?;
        println!("ssh.explicit_owned_session_ciphertext_actor_navigation_eof_sdk_native_join_confirmed auth=2 product_session=false");
        Ok(())
    })
}
