use super::*;
use crate::ssh_session_adapter::route::{self, Confirm, Context};
use pulse_app::local_session_root::SessionRoot;
use russh::server::Server as _;
use russh::{Channel, ChannelId, server};
use std::{
    net::SocketAddr,
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    },
};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;
#[derive(Default)]
struct Metrics {
    active: AtomicUsize,
    auth: AtomicUsize,
    forward: AtomicUsize,
    pty: AtomicUsize,
    shell: AtomicUsize,
}
struct Server {
    metrics: Arc<Metrics>,
    target: Option<SocketAddr>,
    client_key: Option<russh::keys::PublicKey>,
    connection: bool,
    channels: Vec<Channel<server::Msg>>,
}
impl Drop for Server {
    fn drop(&mut self) {
        if self.connection {
            self.metrics.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl server::Server for Server {
    type Handler = Self;
    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        self.metrics.active.fetch_add(1, Ordering::SeqCst);
        Self {
            metrics: self.metrics.clone(),
            target: self.target,
            client_key: self.client_key.clone(),
            connection: true,
            channels: vec![],
        }
    }
}
impl server::Handler for Server {
    type Error = russh::Error;
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        Ok(
            if user == "fixed-user" && self.client_key.as_ref() == Some(key) {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.metrics.auth.fetch_add(1, Ordering::SeqCst);
        self.auth_publickey_offered(user, key).await
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.metrics.auth.fetch_add(1, Ordering::SeqCst);
        Ok(
            if self.client_key.is_none()
                && user == "fixed-user"
                && password == "fixed-native-route-secret"
            {
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn channel_open_session(
        &mut self,
        channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if self.target.is_none() {
            self.channels.push(channel);
            reply.accept().await;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        id: ChannelId,
        _: &str,
        _: u32,
        _: u32,
        _: u32,
        _: u32,
        _: &[(russh::Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.metrics.pty.fetch_add(1, Ordering::SeqCst);
        session.channel_success(id)?;
        Ok(())
    }
    async fn shell_request(
        &mut self,
        id: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        self.metrics.shell.fetch_add(1, Ordering::SeqCst);
        session.channel_success(id)?;
        session.data(id, b"ROUTE_NATIVE_READY\r\n".to_vec())?;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _: &str,
        _: u32,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if let Some(target) = self
            .target
            .filter(|target| host == "127.0.0.1" && port == u32::from(target.port()))
        {
            let mut socket = TcpStream::connect(target).await?;
            reply.accept().await;
            let metrics = self.metrics.clone();
            metrics.forward.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut stream = channel.into_stream();
                let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
                metrics.forward.fetch_sub(1, Ordering::SeqCst);
            });
        }
        Ok(())
    }
}
struct Endpoint {
    address: SocketAddr,
    metrics: Arc<Metrics>,
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::result::Result<(), std::io::Error>>>,
}
impl Drop for Endpoint {
    fn drop(&mut self) {
        self.stop.shutdown("test complete".into());
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn endpoint(
    target: Option<SocketAddr>,
    seed: u8,
    client_key: Option<russh::keys::PublicKey>,
) -> std::result::Result<Endpoint, Box<dyn std::error::Error>> {
    let key = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]).into(),
        "",
    )?;
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let metrics = Arc::new(Metrics::default());
    let mut server = Server {
        metrics: metrics.clone(),
        target,
        client_key,
        connection: false,
        channels: vec![],
    };
    let config = Arc::new(server::Config {
        keys: vec![key],
        inactivity_timeout: Some(Duration::from_secs(120)),
        auth_rejection_time: Duration::from_millis(1),
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let running = server.run_on_socket(config, &listener);
        let _ = tx.send(running.handle());
        running.await
    });
    let stop = rx.await?;
    Ok(Endpoint {
        address,
        metrics,
        stop,
        task: Some(task),
    })
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_two_jump_route_first_pin_cancel_zero_auth_terminal_and_parent_native_exit()
-> TestResult {
    driver_case(false).await
}
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn production_mixed_two_jump_route_uses_private_key_forward_and_password_leaf() -> TestResult
{
    driver_case(true).await
}
async fn driver_case(mixed: bool) -> TestResult {
    let _serial = crate::product_workspace::profiles::VAULT_TEST_SERIAL
        .lock()
        .await;
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let window = registry.open_window()?;
    documents.install(window)?;
    documents.started(window)?;
    let nonce = documents.finished(window)?;
    documents.confirm(window, nonce)?;
    let page = documents.current(window, nonce)?;
    let reader = Arc::new(ProfileReader::new());
    let owner = PageVaultOwner::new(registry.clone());
    let temp = tempfile::tempdir()?;
    let directory = temp.path().join("state");
    for intent in [
        crate::product_workspace::vault::Intent::Create(PasswordInput::new(
            b"fixed-native-route-master".to_vec(),
        )?),
        crate::product_workspace::vault::Intent::Unlock(PasswordInput::new(
            b"fixed-native-route-master".to_vec(),
        )?),
    ] {
        tokio::time::timeout(
            Duration::from_secs(8),
            reader.vault(
                registry.clone(),
                page.clone(),
                directory.clone(),
                owner.clone(),
                intent,
            ),
        )
        .await??;
        eprintln!("fixture.route.vault_stage_finished");
    }
    let store = reader.store.get().ok_or("fixture.store")?.store.clone();
    let vault = store
        .load_vault_ciphertext(Arc::new(|| true))
        .await?
        .vault_id()
        .ok_or("fixture.vault")?;
    let identity = SshPasswordIdentity::new(
        "route-identity",
        "fixed-user",
        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
    )?;
    let identities = store
        .edit_ssh_identities(
            store.load_ssh_identities(Arc::new(|| true)).await?,
            IdentityEdit::Create(identity),
            Arc::new(|| true),
        )
        .await?;
    let identity = identities.records().next().ok_or("fixture.identity")?.id();
    let bank = PasswordWriteBank::new(&owner);
    let ticket = reader
        .password_prepare(
            registry.clone(),
            page.clone(),
            directory.clone(),
            owner.clone(),
            bank.clone(),
            identity,
        )
        .await?;
    reader
        .password_submit(
            registry.clone(),
            page.clone(),
            directory.clone(),
            bank.take(&page, ticket)?,
            SecretBytes::from_owned(b"fixed-native-route-secret".to_vec())?,
        )
        .await?;
    let mut key_identity = None;
    let client_key = if mixed {
        let key = russh::keys::PrivateKey::new(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[23; 32]).into(),
            "",
        )?;
        let pem = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
        let records = store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(Arc::new(|| true)).await?,
                pulse_store::ssh_key_identities::KeyIdentityEdit::Create(
                    pulse_core::ssh_key_identity::SshPrivateKeyIdentity::new(
                        "route-key",
                        "fixed-user",
                        SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
                        None,
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        let id = records.records().next().ok_or("fixture.key")?.id();
        owner
            .begin(page.clone())?
            .prepare_private_key_identity_write(
                store
                    .load_key_identity_vault_source(Arc::new(|| true))
                    .await?,
                id,
            )?
            .write(
                &store,
                SecretBytes::from_owned(pem.as_bytes().to_vec())?,
                None,
                Arc::new(|| true),
            )
            .await?;
        key_identity = Some(id);
        Some(key.public_key().clone())
    } else {
        None
    };
    let leaf_server = endpoint(None, 11, None).await?;
    let middle = endpoint(Some(leaf_server.address), 12, client_key).await?;
    let outer = endpoint(Some(middle.address), 13, None).await?;
    let endpoints = [&outer, &middle, &leaf_server];
    let mut hosts = store.load_ssh_hosts().await?;
    for (name, server) in ["outer", "middle", "leaf"].into_iter().zip(endpoints) {
        hosts = store
            .edit_ssh_hosts(
                hosts,
                HostEdit::Create(SshEndpoint::new(
                    name,
                    "127.0.0.1",
                    server.address.port(),
                    "fixed-user",
                )?),
                Arc::new(|| true),
            )
            .await?;
    }
    let ids: Vec<_> = ["outer", "middle", "leaf"]
        .into_iter()
        .map(|name| {
            hosts
                .records()
                .iter()
                .find(|row| row.endpoint().label() == name)
                .map(|row| row.id())
                .ok_or("fixture.host")
        })
        .collect::<std::result::Result<_, _>>()?;
    let graph = hosts.catalogue().ok_or("fixture.graph")?;
    let profiles: Vec<_> = ids
        .iter()
        .map(|id| {
            graph
                .profiles()
                .iter()
                .find(|p| p.endpoint_id() == *id)
                .map(|p| p.metadata().id())
                .ok_or("fixture.profile")
        })
        .collect::<std::result::Result<_, _>>()?;
    let mut snapshot = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    for (index, id) in profiles.iter().enumerate() {
        snapshot = store
            .edit_ssh_profile_with_jump(
                snapshot,
                ProfileEdit {
                    id: *id,
                    label: format!("role-{index}"),
                    username: "fixed-user".into(),
                    identity: Some(if index == 1 && mixed {
                        key_identity.ok_or("fixture.key")?
                    } else {
                        identity
                    }),
                },
                index.checked_sub(1).map(|previous| profiles[previous]),
                Arc::new(|| true),
            )
            .await?;
    }
    let root = Arc::new(Mutex::new(SessionRoot::new(
        registry.clone(),
        tokio::runtime::Handle::current(),
    )));
    let size = pulse_core::TermSize::new(80, 24)?;
    let make_context = || Context {
        broker: None,
        store: store.clone(),
        registry: registry.clone(),
        page: page.clone(),
        owner: owner.clone(),
        root: root.clone(),
        size,
        history_rows: 2000,
        palette: pulse_term::QueryPalette::default().into(),
    };
    let other_window = registry.open_window()?;
    documents.install(other_window)?;
    documents.started(other_window)?;
    let other_nonce = documents.finished(other_window)?;
    documents.confirm(other_window, other_nonce)?;
    let other_page = documents.current(other_window, other_nonce)?;
    route::requests::tests::verify_bank(&owner, page.clone(), other_page, ids[2], identity).await?;
    documents.remove(other_window)?;
    registry.close_window(other_window)?;
    let read = || {
        let captured = owner.begin_routed_session_gesture_authorized(
            page.clone(),
            ids[2],
            identity,
            Arc::new(|| true),
        );
        let reader = reader.clone();
        let registry = registry.clone();
        let page = page.clone();
        let directory = directory.clone();
        let profile = profiles[2];
        async move {
            let captured = captured.map_err(|_| "fixture.capture")?;
            reader
                .ssh_route(
                    registry,
                    page,
                    directory,
                    captured,
                    profile,
                    SshRouteIdentity::Password(identity),
                )
                .await
        }
    };
    let (_, route) = read().await?;
    let declined: Confirm = Arc::new(|prompt| {
        assert!(prompt.first_use);
        assert_eq!(prompt.index, 0);
        Box::pin(async { Ok(route::Decision::Approved(false)) })
    });
    assert!(route::run(make_context(), route, declined).await.is_err());
    assert_eq!(
        store
            .load_ssh_target_source(Arc::new(|| true))
            .await?
            .trust()
            .revision(),
        0
    );
    for server in endpoints {
        assert_eq!(server.metrics.auth.load(Ordering::SeqCst), 0);
    }
    let prompts = Arc::new(Mutex::new(vec![]));
    let saved = prompts.clone();
    let confirm: Confirm = Arc::new(move |prompt| {
        if let Ok(mut rows) = saved.lock() {
            rows.push((prompt.index, prompt.count, prompt.first_use));
        }
        Box::pin(async { Ok(route::Decision::Approved(true)) })
    });
    let (_, route) = read().await?;
    let id = tokio::time::timeout(
        Duration::from_secs(10),
        route::run(make_context(), route, confirm),
    )
    .await??;
    route::requests::tests::verify_done_consumption(&owner, page.clone(), ids[2], identity, id)
        .await?;
    assert_eq!(
        *prompts.lock().map_err(|_| "fixture.lock")?,
        vec![(0, 3, true), (1, 3, true), (2, 3, true)]
    );
    assert_eq!(
        store
            .load_ssh_target_source(Arc::new(|| true))
            .await?
            .trust()
            .revision(),
        3
    );
    for server in endpoints {
        assert_eq!(server.metrics.auth.load(Ordering::SeqCst), 1);
    }
    assert_eq!(outer.metrics.pty.load(Ordering::SeqCst), 0);
    assert_eq!(middle.metrics.pty.load(Ordering::SeqCst), 0);
    assert_eq!(leaf_server.metrics.shell.load(Ordering::SeqCst), 1);
    let cleanup = root
        .lock()
        .map_err(|_| "fixture.lock")?
        .close_protocol(id)?;
    tokio::time::timeout(Duration::from_secs(5), cleanup.join()).await??;
    tokio::time::timeout(Duration::from_secs(5), async {
        while endpoints.iter().any(|server| {
            server.metrics.active.load(Ordering::SeqCst) != 0
                || server.metrics.forward.load(Ordering::SeqCst) != 0
        }) {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await?;
    reader.shutdown().await?;
    Ok(())
}
