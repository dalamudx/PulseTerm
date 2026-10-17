//! Fixed loopback and real shared terminal adapter; backend probe, not real WebView acceptance.
use super::*;
use pulse_app::{
    local_session_root::SessionRoot,
    protocol_session_owner::ProtocolSessionReservation,
    terminal_view::{TerminalViewBinding as TermView, TerminalViewDelivery},
};
use pulse_core::TermSize;
use pulse_term::session::{Lifecycle, ResizeBasis};
use russh::{Channel, ChannelId, Pty};
struct SessionSpy {
    auth: Spy,
    pty: bool,
    pending_setup: Arc<std::sync::atomic::AtomicBool>,
    setup: Arc<tokio::sync::Notify>,
    resized: tokio::sync::watch::Sender<Option<(u32, u32)>>,
}
impl Server for SessionSpy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            auth: Spy {
                counts: self.auth.counts.clone(),
                allowed: self.auth.allowed.clone(),
                session: true,
            },
            pty: false,
            pending_setup: self.pending_setup.clone(),
            setup: self.setup.clone(),
            resized: self.resized.clone(),
        }
    }
}
impl server::Handler for SessionSpy {
    type Error = russh::Error;
    async fn auth_none(&mut self, user: &str) -> std::result::Result<server::Auth, Self::Error> {
        server::Handler::auth_none(&mut self.auth, user).await
    }
    async fn auth_password(
        &mut self,
        user: &str,
        secret: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        server::Handler::auth_password(&mut self.auth, user, secret).await
    }
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        server::Handler::auth_publickey_offered(&mut self.auth, user, key).await
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        server::Handler::auth_publickey(&mut self.auth, user, key).await
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        columns: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.pty = term == "xterm-256color" && columns == 32 && rows == 8;
        self.setup.notify_one();
        if self.pending_setup.load(Ordering::Acquire) {
            return Ok(());
        }
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
    ) -> std::result::Result<(), Self::Error> {
        self.resized.send_replace(Some((columns, rows)));
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if !self.pty {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        session.data(channel, b"KEY_ROOT_READY\r\n".to_vec())?;
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if data.len() > 128 || !self.pty {
            return Err(russh::Error::Disconnect);
        }
        if data == b"exit" {
            session.data(channel, b"KEY_ROOT_END\r\n".to_vec())?;
            session.exit_status_request(channel, 7)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            let mut echo = b"KEY_ROOT_ECHO:".to_vec();
            echo.extend_from_slice(data);
            echo.extend_from_slice(b"\r\n");
            session.data(channel, echo)?;
        }
        Ok(())
    }
}
async fn connect(
    target: &HostTarget,
    key: &PrivateKey,
    address: std::net::SocketAddr,
) -> Result<PinnedTransport> {
    Ok(PinnedTransport::connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target.clone(), key.public_key().clone())]),
        Some(vec![]),
        Arc::new(russh::client::Config::default()),
        tokio::net::TcpStream::connect(address).await?,
    )
    .await?)
}
async fn selected(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    store: &StateStore,
    host: Uuid,
    identity: Uuid,
    transport: PinnedTransport,
) -> Result<(
    pulse_app::vault_owner::PrivateKeySessionIntent,
    adapter::session::SessionSink,
)> {
    let source = store.load_ssh_key_target_source(Arc::new(|| true)).await?;
    let guard = adapter::peer_guard(&source, host, &transport)?;
    let intent = owner
        .begin_private_key_session_action_authorized(page, host, identity, guard)?
        .prepare(store, source)
        .await?;
    Ok((
        intent,
        adapter::session::SessionSink::new(
            transport,
            (host, identity),
            TermSize::new(32, 8)?,
            128,
        )?,
    ))
}
#[path = "ssh_private_key_pem_fixture.rs"]
mod pem_fixture;
#[derive(Clone, Copy)]
enum KeySource {
    OpenSsh,
    Pkcs8,
}
pub(super) async fn run_async() -> Result<()> {
    run_key(
        "ed25519",
        PrivateKey::new(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
            "",
        )?,
        KeySource::OpenSsh,
    )
    .await
}
pub(super) async fn run_algorithms() -> Result<()> {
    use russh::keys::{Algorithm, EcdsaCurve};
    for (name, curve) in [
        ("nistp256", EcdsaCurve::NistP256),
        ("nistp384", EcdsaCurve::NistP384),
        ("nistp521", EcdsaCurve::NistP521),
    ] {
        for source in [KeySource::OpenSsh, KeySource::Pkcs8] {
            run_key(
                name,
                PrivateKey::random(&mut rand::rng(), Algorithm::Ecdsa { curve })?,
                source,
            )
            .await?;
        }
    }
    Ok(())
}
async fn run_key(algorithm: &str, user_key: PrivateKey, source: KeySource) -> Result<()> {
    let report = crate::root()?.join(if algorithm == "ed25519" {
        "target/probes/ssh-private-key-session.json".into()
    } else {
        let suffix = if matches!(source, KeySource::Pkcs8) {
            "-pkcs8"
        } else {
            ""
        };
        format!("target/probes/ssh-private-key-session-{algorithm}{suffix}.json")
    });
    if report.exists() {
        std::fs::remove_file(&report)?;
    }
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    let owner = PageVaultOwner::new(registry.clone());
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    let vault = Uuid::now_v7();
    let (envelope, mut cipher) =
        tokio::task::spawn_blocking(move || create_validation_vault(MASTER, vault)).await??;
    let saved = store
        .edit_vault_ciphertext(
            store.load_vault_ciphertext(Arc::new(|| true)).await?,
            VaultEdit::Initialize(vault, envelope),
            Arc::new(|| true),
        )
        .await?;
    let op = owner.begin(page.clone())?;
    let unlock = op.begin_unlock(PasswordInput::new(MASTER.to_vec())?, saved.clone())?;
    op.confirm_unlock(&store, saved, unlock.ready().await?)
        .await?
        .accept()?;
    let pem = match source {
        KeySource::OpenSsh => user_key
            .encrypt(&mut rand::rng(), PASS)?
            .to_openssh(LineEnding::LF)?,
        KeySource::Pkcs8 => pem_fixture::encrypted(&user_key, PASS)?,
    };
    let key_id = Uuid::now_v7();
    let pass_id = Uuid::now_v7();
    for record in [
        seal(
            &mut cipher,
            vault,
            key_id,
            SecretKind::SshPrivateKey,
            pem.as_bytes(),
        )?,
        seal(
            &mut cipher,
            vault,
            pass_id,
            SecretKind::SshKeyPassphrase,
            PASS,
        )?,
    ] {
        store
            .edit_vault_ciphertext(
                store.load_vault_ciphertext(Arc::new(|| true)).await?,
                VaultEdit::Create(record),
                Arc::new(|| true),
            )
            .await?;
    }
    let identities = store
        .edit_ssh_key_identities(
            store.load_ssh_key_identities(Arc::new(|| true)).await?,
            KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                "fixed-key",
                "user",
                SecretRef::new(vault, key_id, SecretKind::SshPrivateKey, false)?,
                Some(SecretRef::new(
                    vault,
                    pass_id,
                    SecretKind::SshKeyPassphrase,
                    false,
                )?),
            )?),
            Arc::new(|| true),
        )
        .await?;
    let identity = identities
        .records()
        .next()
        .ok_or_else(|| anyhow::anyhow!("fixture.identity"))?
        .id();
    // Reopen ciphertext and identity through a fresh worker after the original worker exits.
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    ensure!(
        store
            .load_ssh_key_identities(Arc::new(|| true))
            .await?
            .records()
            .any(|row| row.id() == identity),
        "fixture.key_reopened"
    );
    // Reopening does not upgrade the previous reader's accepted-key authority.
    owner.close()?;
    let owner = PageVaultOwner::new(registry.clone());
    let reopened = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    let operation = owner.begin(page.clone())?;
    let unlock = operation.begin_unlock(PasswordInput::new(MASTER.to_vec())?, reopened.clone())?;
    operation
        .confirm_unlock(&store, reopened, unlock.ready().await?)
        .await?
        .accept()?;
    let key = PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7; 32]).into(),
        "",
    )?;
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let address = listener.local_addr()?;
    let target = HostTarget::new("127.0.0.1", address.port())?;
    let counts = Arc::new(Counts::default());
    let pending_setup = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let setup = Arc::new(tokio::sync::Notify::new());
    let (resized, mut remote_size) = tokio::sync::watch::channel(None);
    let config = Arc::new(server::Config {
        keys: vec![key.clone()],
        ..Default::default()
    });
    let (mut spy, tx) = (
        SessionSpy {
            auth: Spy {
                counts: counts.clone(),
                allowed: user_key.public_key().clone(),
                session: false,
            },
            pty: false,
            pending_setup: pending_setup.clone(),
            setup: setup.clone(),
            resized,
        },
        tokio::sync::oneshot::channel(),
    );
    let (receipt, receiver) = tx;
    let task = tokio::spawn(async move {
        let running = spy.run_on_socket(config, &listener);
        let _ = receipt.send(running.handle());
        running.await
    });
    let mut server = ServerGuard {
        stop: receiver.await?,
        task: Some(task),
    };
    let mut sessions = SessionRoot::new(registry.clone(), tokio::runtime::Handle::current());
    let result: Result<()> = async {
        let hosts = store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new(
                    "fixed-host",
                    "127.0.0.1",
                    address.port(),
                    "user",
                )?),
                Arc::new(|| true),
            )
            .await?;
        let host = hosts.records()[0].id();
        store
            .edit_ssh_trust(
                store.load_ssh_trust().await?,
                pin_edit(&target, key.public_key())?,
            )
            .await?;
        // Change the complete source after the original intent: not one signature may be sent.
        let transport = connect(&target, &key, address).await?;
        let native = transport.native_io_completion();
        let started = ProtocolSessionReservation::reserve(&registry, &page)?.begin()?;
        let (intent, sink) =
            selected(&owner, page.clone(), &store, host, identity, transport).await?;
        store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new("other", "other.example", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        ensure!(
            intent
                .start(store.clone(), sink, started)
                .wait()
                .await
                .is_err(),
            "fixture.stale_source_accepted"
        );
        native.wait().await?;
        wait_closed(&counts, 1).await?;
        ensure!(
            counts.signed.load(Ordering::SeqCst) == 0 && counts.offered.load(Ordering::SeqCst) == 0,
            "fixture.stale_source_auth_sent"
        );
        // Revoke the actual document while the real peer withholds its terminal-setup reply.
        pending_setup.store(true, Ordering::Release);
        let transport = connect(&target, &key, address).await?;
        let native = transport.native_io_completion();
        let started = ProtocolSessionReservation::reserve(&registry, &page)?.begin()?;
        let (intent, sink) =
            selected(&owner, page.clone(), &store, host, identity, transport).await?;
        let admission = sink.observe_native_admission();
        let task = intent.start(store.clone(), sink, started);
        tokio::time::timeout(std::time::Duration::from_secs(2), setup.notified()).await?;
        documents.started(window)?;
        owner.retire_page()?;
        ensure!(task.wait().await.is_err(), "fixture.revoked_setup_accepted");
        native.wait().await?;
        wait_closed(&counts, 2).await?;
        ensure!(
            !admission() && sessions.occupied() == 0,
            "fixture.setup_native_retained"
        );
        let nonce = documents.finished(window)?;
        documents.confirm(window, nonce)?;
        let current = documents.current(window, nonce)?;
        pending_setup.store(false, Ordering::Release);
        // Accepted root transport survives page changes and retains its physical resource admission.
        let transport = connect(&target, &key, address).await?;
        let native = transport.native_io_completion();
        let peer = transport.peer()?;
        let started = ProtocolSessionReservation::reserve(&registry, &current)?.begin()?;
        let (intent, sink) =
            selected(&owner, current.clone(), &store, host, identity, transport).await?;
        let admission = sink.observe_native_admission();
        let owned = intent.start(store.clone(), sink, started).wait().await?;
        let handle = owned.handle().clone();
        let mut state = handle.state.clone();
        let id = sessions.adopt_protocol(owned)?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.wait_for(|s| s.parsed_bytes >= 16),
        )
        .await??;
        let boot = handle.diagnostic_bootstrap(0, 1024, 8192).await?;
        ensure!(
            boot.restore
                .windows(b"KEY_ROOT_READY".len())
                .any(|w| w == b"KEY_ROOT_READY"),
            "fixture.ready_actor"
        );
        let view = TermView::attach_v2(
            &registry,
            current.clone(),
            id,
            pulse_app::attachment::AttachmentConfig::default(),
        )
        .await?;
        let delivery = TerminalViewDelivery::new(view)?;
        let view = delivery.binding();
        let requested = TermSize::new(64, 16)?;
        let layout = tokio::time::timeout(
            std::time::Duration::from_secs(2),
            view.resize_with_receipt(requested),
        )
        .await??;
        ensure!(
            layout.size == requested && layout.basis == Some(ResizeBasis::SshWindowRequestAdmitted),
            "fixture.resize_basis"
        );
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            remote_size.wait_for(|s| *s == Some((64, 16))),
        )
        .await??;
        ensure!(
            view.input(b"old-view-denied".to_vec()).await.is_err(),
            "fixture.resize_old_input_enabled"
        );
        drop(view);
        drop(delivery);
        documents.started(window)?;
        owner.retire_page()?;
        ensure!(
            peer.is_current() && admission() && sessions.ids() == vec![id],
            "fixture.navigation_closed_root"
        );
        let nonce = documents.finished(window)?;
        documents.confirm(window, nonce)?;
        let current = documents.current(window, nonce)?;
        handle.input(b"phase-b".to_vec()).await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.wait_for(|s| s.parsed_bytes >= 39),
        )
        .await??;
        let boot = handle.diagnostic_bootstrap(0, 1024, 8192).await?;
        ensure!(
            boot.restore
                .windows(b"KEY_ROOT_ECHO:phase-b".len())
                .any(|w| w == b"KEY_ROOT_ECHO:phase-b"),
            "fixture.echo_actor"
        );
        handle.input(b"exit".to_vec()).await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.wait_for(|s| s.lifecycle == Lifecycle::Ended),
        )
        .await??;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            sessions.take_protocol_for_finish(id)?.finish(),
        )
        .await??;
        ensure!(
            sessions.collect_reaped() == 1 && sessions.occupied() == 0,
            "fixture.eof_capacity"
        );
        native.wait().await?;
        wait_closed(&counts, 3).await?;
        ensure!(
            !peer.is_current() && !admission(),
            "fixture.eof_native_retained"
        );
        // Explicit close while the actual SDK reader is waiting must also drain native resources.
        let transport = connect(&target, &key, address).await?;
        let native = transport.native_io_completion();
        let started = ProtocolSessionReservation::reserve(&registry, &current)?.begin()?;
        let (intent, sink) =
            selected(&owner, current.clone(), &store, host, identity, transport).await?;
        let admission = sink.observe_native_admission();
        let owned = intent.start(store.clone(), sink, started).wait().await?;
        let id = sessions.adopt_protocol(owned)?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            sessions.close_protocol(id)?.join(),
        )
        .await??;
        native.wait().await?;
        wait_closed(&counts, 4).await?;
        ensure!(
            sessions.collect_reaped() == 1 && sessions.occupied() == 0 && !admission(),
            "fixture.close_native_retained"
        );
        ensure!(
            counts.signed.load(Ordering::SeqCst) == 3
                && counts.offered.load(Ordering::SeqCst) == 3
                && counts.other_auth.load(Ordering::SeqCst) == 0,
            "fixture.auth_counts"
        );
        // The actual production request bank and supervisor, not a direct authentication action.
        let bank = crate::ssh_session_adapter::requests::SessionRequests::new(
            registry.clone(),
            window,
            tokio::runtime::Handle::current(),
        );
        let root = Arc::new(std::sync::Mutex::new(SessionRoot::new(
            registry.clone(),
            tokio::runtime::Handle::current(),
        )));
        let gesture = owner.begin_private_key_session_gesture_authorized(
            current.clone(),
            host,
            identity,
            Arc::new(|| true),
        )?;
        let competing = owner.begin_password_session_gesture_authorized(
            current.clone(),
            host,
            identity,
            Arc::new(|| true),
        )?;
        let ticket = bank
            .begin_key_gesture(current.clone(), host, identity, &owner, gesture)
            .map_err(|e| anyhow::anyhow!(e))?;
        ensure!(
            bank.begin_gesture(current.clone(), host, identity, &owner, competing)
                .is_err(),
            "fixture.shared_start_slot_reused"
        );
        let polled = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = polled.clone();
        ensure!(
            bank.prepare(current.clone(), ticket, async move {
                observed.store(true, Ordering::SeqCst);
                Err::<(StateStore, pulse_store::ssh_target_source::SshTargetSource), _>(
                    "fixture.wrong_role_polled",
                )
            })
            .await
            .is_err()
                && !polled.load(Ordering::SeqCst),
            "fixture.wrong_role_source_polled"
        );
        let source_store = store.clone();
        let guard = bank
            .authorization(&current, ticket)
            .map_err(|e| anyhow::anyhow!(e))?;
        bank.prepare_key(current.clone(), ticket, async move {
            let source = source_store
                .load_ssh_key_target_source(guard)
                .await
                .map_err(|_| "fixture.key_source_failed")?;
            Ok((source_store, source))
        })
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
        let id = bank
            .start(
                current.clone(),
                ticket,
                crate::ssh_session_adapter::requests::StartTarget {
                    root: root.clone(),
                    size: TermSize::new(32, 8)?,
                    history_rows: 128,
                    palette: pulse_term::QueryPalette::default().into(),
                },
            )
            .await
            .map_err(|e| anyhow::anyhow!(e))?;
        let handle = root
            .lock()
            .map_err(|_| anyhow::anyhow!("fixture.root_lock"))?
            .protocol_handle(id)?;
        let mut state = handle.state.clone();
        handle.input(b"bank-echo".to_vec()).await?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            state.wait_for(|s| s.parsed_bytes >= 38),
        )
        .await??;
        let boot = handle.diagnostic_bootstrap(0, 1024, 8192).await?;
        ensure!(
            boot.restore
                .windows(b"KEY_ROOT_ECHO:bank-echo".len())
                .any(|w| w == b"KEY_ROOT_ECHO:bank-echo"),
            "fixture.product_bank_echo_missing"
        );
        let cleanup = root
            .lock()
            .map_err(|_| anyhow::anyhow!("fixture.root_lock"))?
            .close_protocol(id)?;
        tokio::time::timeout(std::time::Duration::from_secs(2), cleanup.join()).await??;
        wait_closed(&counts, 5).await?;
        bank.shutdown().await.map_err(|e| anyhow::anyhow!(e))?;
        let root_drain = root
            .lock()
            .map_err(|_| anyhow::anyhow!("fixture.root_lock"))?
            .shutdown();
        root_drain.join().await?;
        ensure!(
            counts.signed.load(Ordering::SeqCst) == 4
                && counts.offered.load(Ordering::SeqCst) == 4
                && counts.other_auth.load(Ordering::SeqCst) == 0,
            "fixture.product_bank_auth_counts"
        );
        Ok(())
    }
    .await;
    let owner_result = owner.close();
    let drain = sessions.shutdown();
    let root_result = drain.join().await;
    let native_result = registry.seal_native_workers().join().await;
    drop(store);
    let worker_result = worker.join().await;
    server.stop.shutdown("fixed key-session end".into());
    let server_result: Result<()> = if let Some(mut task) = server.task.take() {
        match tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await {
            Ok(joined) => joined
                .map_err(|_| anyhow::anyhow!("fixture.server_join"))?
                .map_err(anyhow::Error::from),
            Err(_) => {
                task.abort();
                let _ = task.await;
                Err(anyhow::anyhow!("fixture.server_exit"))
            }
        }
    } else {
        Ok(())
    };
    let registry_result = registry.shutdown();
    result?;
    owner_result?;
    root_result?;
    native_result?;
    worker_result?;
    server_result?;
    registry_result?;
    std::fs::create_dir_all(
        report
            .parent()
            .ok_or_else(|| anyhow::anyhow!("fixture.report_parent"))?,
    )?;
    std::fs::write(
        report,
        serde_json::to_vec_pretty(
            &serde_json::json!({"schemaVersion":2,"algorithm":algorithm,"sourceFormat":if matches!(source, KeySource::Pkcs8) { "encrypted-pkcs8" } else { "encrypted-openssh" },"ciphertextWorkerReopened":true,"signedAuthentications":4,"productBankWrongRoleRejectedBeforeSourcePoll":true,"productBankSharedAdmission":true,"productBankPrivateKeyStartupAndNativeExit":true,"staleSourceNoAuthentication":true,"setupPageRevocationJoined":true,"encryptedKeyCiphertextAuthenticated":true,"sameSharedRootAdopted":true,"rootSurvivedPageNavigation":true,"inputEchoAfterNavigation":true,"sshWindowRequestAdmitted":true,"serverReceivedWindowRequest":true,"remoteKernelResizeConfirmed":false,"orderedEof":true,"closeWhileNativeReadPending":true,"sdkAndNativeJoined":true,"plaintextReturned":false,"realWebViewChecked":false,"productEntryOpened":false}),
        )?,
    )?;
    println!(
        "ssh.private_key_session_ciphertext_root_navigation_resize_echo_eof_close_native_exit_confirmed"
    );
    Ok(())
}
