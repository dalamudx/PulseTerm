//! Isolated upper protocol adapter; fixed synthetic data, never user credentials or a production SSH entry.
use super::*;
#[path = "ssh_owned_session_adapter.rs"]
mod owned_adapter;
#[path = "ssh_owned_session_probe.rs"]
pub(crate) mod owned_session;
use pulse_app::vault_owner::{PasswordTargetSink, VaultOwnerError};
use pulse_ssh::peer::{NativeIoCompletion, VerifiedPeer};
use std::{future::Future, pin::Pin, sync::Mutex};
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
struct Admission {
    guard: Mutex<Option<Guard>>,
}
struct Sink {
    transport: Option<PinnedTransport>,
    peer: VerifiedPeer,
    selected: (Uuid, Uuid),
    admission: Arc<Admission>,
    completion: Option<NativeIoCompletion>,
}
impl Sink {
    fn new(mut transport: PinnedTransport, selected: (Uuid, Uuid)) -> Result<Self> {
        let peer = transport.peer()?;
        let admission = Arc::new(Admission {
            guard: Mutex::new(None),
        });
        let completion = transport.retain_native_admission(&peer, admission.clone())?;
        Ok(Self {
            transport: Some(transport),
            peer,
            selected,
            admission,
            completion: Some(completion),
        })
    }
}
impl PasswordTargetSink for Sink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
            && source
                .identity_vault()
                .identities()
                .records()
                .any(|row| row.id() == identity)
            && self
                .transport
                .as_ref()
                .is_some_and(|t| peer_guard(source, host, t).is_ok_and(|guard| guard()))
    }
    fn retain_native_authorization(
        &mut self,
        guard: Guard,
    ) -> std::result::Result<(), VaultOwnerError> {
        let mut state = self
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?;
        if state.is_some() {
            return Err(VaultOwnerError::Denied);
        }
        *state = Some(guard);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        Box::pin(async move {
            let transport = self.transport.as_mut().ok_or(VaultOwnerError::Denied)?;
            let client = transport
                .client(&self.peer)
                .map_err(|_| VaultOwnerError::Denied)?;
            let result = client
                .authenticate_password(user, password)
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            Ok(result.success())
        })
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            drop(self.transport.take());
            self.completion
                .take()
                .ok_or(VaultOwnerError::Failed)?
                .wait()
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            let held = self
                .admission
                .guard
                .lock()
                .map_err(|_| VaultOwnerError::Failed)?
                .take();
            drop(held);
            Ok(())
        })
    }
}
struct AcceptSpy {
    auth: Arc<AtomicUsize>,
    exact: Arc<std::sync::atomic::AtomicBool>,
}
impl Server for AcceptSpy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            auth: self.auth.clone(),
            exact: self.exact.clone(),
        }
    }
}
impl server::Handler for AcceptSpy {
    type Error = russh::Error;
    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        let exact = user == "user" && password == "fixed-target-secret";
        self.exact.store(exact, Ordering::Release);
        Ok(if exact {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
}
pub(crate) fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(4).build()?.block_on(async{
 let registry=SessionRegistry::new();let documents=ProductDocuments::new(registry.clone());let window=registry.open_window()?;documents.install(window)?;documents.started(window)?;let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;let page=documents.current(window,nonce)?;let owner=PageVaultOwner::new(registry.clone());let root=tempfile::tempdir()?;let dir=pulse_store::state_directory::prepare(&root.path().join("state"))?;let(store,worker)=StateStore::open(dir.join("state.db")).await?;
 let empty=store.load_vault_ciphertext(Arc::new(||true)).await?;let op=owner.begin(page.clone())?;let(vault,request)=op.begin_creation(PasswordInput::new(b"fixed-target-master".to_vec())?,empty.clone())?;op.persist_creation(&store,empty,vault,request.ready().await?).await?;let saved=store.load_vault_ciphertext(Arc::new(||true)).await?;let request=op.begin_unlock(PasswordInput::new(b"fixed-target-master".to_vec())?,saved.clone())?;op.confirm_unlock(&store,saved,request.ready().await?).await?.accept()?;
 let value=SshPasswordIdentity::new("fixed","user",SecretRef::new(vault,Uuid::now_v7(),SecretKind::SshPassword,false)?)?;let meta=store.edit_ssh_identities(store.load_ssh_identities(Arc::new(||true)).await?,IdentityEdit::Create(value),Arc::new(||true)).await?;let identity=meta.records().next().ok_or_else(||anyhow::anyhow!("fixture.identity"))?.id();owner.begin(page.clone())?.prepare_password_identity_write(store.load_identity_vault_source(Arc::new(||true)).await?,identity)?.write(&store,SecretBytes::from_owned(b"fixed-target-secret".to_vec())?).await?;
 let key=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7;32]).into(),"")?;let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await?;let address=listener.local_addr()?;let target=HostTarget::new("127.0.0.1",address.port())?;let auth=Arc::new(AtomicUsize::new(0));let exact=Arc::new(std::sync::atomic::AtomicBool::new(false));let spy=AcceptSpy{auth:auth.clone(),exact:exact.clone()};let config=Arc::new(server::Config{keys:vec![key.clone()],..Default::default()});let(tx,rx)=tokio::sync::oneshot::channel();let task=tokio::spawn(async move{let mut spy=spy;let running=spy.run_on_socket(config,&listener);let _sent=tx.send(running.handle());running.await});let mut server=ServerGuard{stop:rx.await?,task:Some(task)};
 let hosts=store.edit_ssh_hosts(store.load_ssh_hosts().await?,HostEdit::Create(SshEndpoint::new("fixed","127.0.0.1",address.port(),"user")?),Arc::new(||true)).await?;let host=hosts.records()[0].id();let _trust=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,key.public_key())?).await?;let source=store.load_ssh_target_source(Arc::new(||true)).await?;
 let bad=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[8;32]).into(),"")?;
 for pins in [vec![],vec![HostPin::new(target.clone(),bad.public_key().clone())]]{ensure!(PinnedTransport::connect_stream(target.clone(),Some(pins),None,Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await.is_err(),"fixture.unknown_or_changed_key_not_rejected");ensure!(auth.load(Ordering::SeqCst)==0,"fixture.pre_auth_not_zero");}
 let connection=PinnedTransport::connect_stream(target.clone(),Some(validated_pins(source.trust())?),None,Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await?;
 let gate=peer_guard(&source,host,&connection)?;let stale=owner.begin_password_target_action_authorized(page.clone(),host,identity,gate)?.prepare(&store,source.clone()).await?;let stale_sink=Sink::new(connection,(host,identity))?;let stale_weak=Arc::downgrade(&stale_sink.admission);let _changed=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,bad.public_key())?).await?;
 ensure!(stale.start(store.clone(),stale_sink)?.wait().await.is_err(),"fixture.stale_source_accepted");ensure!(auth.load(Ordering::SeqCst)==0&&stale_weak.upgrade().is_none(),"fixture.stale_source_auth_or_native_credit");
 let _restore=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,key.public_key())?).await?;let source=store.load_ssh_target_source(Arc::new(||true)).await?;
 let wrong=HostTarget::new("wrong.fixed.invalid",address.port())?;let connection=PinnedTransport::connect_stream(wrong.clone(),Some(vec![HostPin::new(wrong,key.public_key().clone())]),None,Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await?;let wrong_proof=connection.peer()?;let wrong_io=connection.native_io_completion();let action=owner.begin_password_target_action_authorized(page.clone(),host,identity,Arc::new(move||wrong_proof.is_current()))?.prepare(&store,source.clone()).await?;ensure!(action.start(store.clone(),Sink::new(connection,(host,identity))?).is_err(),"fixture.wrong_actual_target_accepted");tokio::time::timeout(std::time::Duration::from_secs(2),wrong_io.wait()).await??;ensure!(auth.load(Ordering::SeqCst)==0,"fixture.wrong_target_authenticated");
 let transport=PinnedTransport::connect_stream(target.clone(),Some(validated_pins(source.trust())?),None,Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await?;let guard=peer_guard(&source,host,&transport)?;let proof=transport.peer()?;let sink=Sink::new(transport,(host,identity))?;let weak=Arc::downgrade(&sink.admission);ensure!(auth.load(Ordering::SeqCst)==0,"fixture.pre_auth_not_zero");
 let action=owner.begin_password_target_action_authorized(page.clone(),host,identity,guard)?.prepare(&store,source).await?;ensure!(action.start(store.clone(),sink)?.wait().await?,"fixture.auth_rejected");ensure!(auth.load(Ordering::SeqCst)==1&&exact.load(Ordering::Acquire),"fixture.auth_bytes_or_count");ensure!(!proof.is_current()&&weak.upgrade().is_none(),"fixture.native_admission_not_drained");
 let source=store.load_ssh_target_source(Arc::new(||true)).await?;let connection=PinnedTransport::connect_stream(target.clone(),Some(validated_pins(source.trust())?),None,Arc::new(russh::client::Config::default()),tokio::net::TcpStream::connect(address).await?).await?;let gate=peer_guard(&source,host,&connection)?;let io=connection.native_io_completion();let retired=owner.begin_password_target_action_authorized(page.clone(),host,identity,gate)?.prepare(&store,source).await?;documents.started(window)?;owner.retire_page()?;ensure!(retired.start(store.clone(),Sink::new(connection,(host,identity))?).is_err(),"fixture.retired_action_accepted");tokio::time::timeout(std::time::Duration::from_secs(2),io.wait()).await??;ensure!(auth.load(Ordering::SeqCst)==1,"fixture.retired_action_authenticated");
 owner.close()?;drop(store);worker.join().await?;registry.shutdown()?;server.stop.shutdown("fixed end".into());if let Some(task)=server.task.take(){tokio::time::timeout(std::time::Duration::from_secs(2),task).await???;}
 let output=crate::root()?.join("target/probes/ssh-password-use.json");std::fs::create_dir_all(output.parent().ok_or_else(||anyhow::anyhow!("fixture.output"))?)?;std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"schemaVersion":1,"explicitActionConfirmed":true,"ciphertextAuthenticated":true,"actualPeerBound":true,"nativeAdmissionDrained":true,"unknownAndWrongKeyPreAuthRejected":true,"changedPersistedSourceRejected":true,"wrongActualTargetRejected":true,"retiredPageRejected":true,"authenticationRequests":1,"exactFixedBytes":true,"plaintextReturned":false,"productSshOpened":false}))?)?;println!("ssh.explicit_password_action_actual_peer_auth_native_drain_confirmed auth=1 plaintext_returned=false");Ok(())})
}
