//! Upper validation adapter: app remains network-free. No user SSH sources, credential read or auth.
use anyhow::{Result, ensure};
#[path = "ssh_first_pin_probe.rs"]
pub(crate) mod first_pin;
#[path = "ssh_password_use_probe.rs"]
pub(crate) mod password_use;
#[cfg(feature = "ssh-private-key-probe")]
#[path = "ssh_private_key_use_probe.rs"]
pub(crate) mod private_key_use;
use pulse_app::{
    product_document::ProductDocuments,
    registry::{PageLaunchScope, SessionRegistry},
    vault_owner::{PageVaultOwner, SavedPasswordUsePreparation},
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_identity::SshPasswordIdentity,
};
use pulse_ssh::{
    HostPin, HostTarget,
    managed_store::{pin_edit, validated_pins},
    peer::PinnedTransport,
    verify_host_key,
};
use pulse_store::{
    StateStore, ssh_hosts::HostEdit, ssh_identities::IdentityEdit,
    ssh_target_source::SshTargetSource, vault_crypto::SecretBytes, vault_unlock::PasswordInput,
};
use russh::server::{self, Server};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
struct Spy {
    auth: Arc<AtomicUsize>,
}
impl Server for Spy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            auth: self.auth.clone(),
        }
    }
}
impl server::Handler for Spy {
    type Error = russh::Error;
    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_password(&mut self, _: &str, _: &str) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
}
struct ServerGuard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.shutdown("fixed end".into());
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
fn peer_guard(
    source: &SshTargetSource,
    host: Uuid,
    transport: &PinnedTransport,
) -> Result<Arc<dyn Fn() -> bool + Send + Sync>> {
    let peer = transport.peer()?;
    let row = source
        .hosts()
        .records()
        .iter()
        .find(|row| row.id() == host)
        .ok_or_else(|| anyhow::anyhow!("fixture.host"))?;
    ensure!(
        row.endpoint().target().host() == peer.target().host()
            && row.endpoint().target().port() == peer.target().port(),
        "fixture.wrong_target"
    );
    let pins = validated_pins(source.trust())?;
    verify_host_key(peer.target(), peer.public_key(), Some(&pins), None)?;
    Ok(Arc::new(move || peer.is_current()))
}
async fn prepare(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    store: &StateStore,
    source: SshTargetSource,
    host: Uuid,
    identity: Uuid,
    transport: &PinnedTransport,
) -> Result<SavedPasswordUsePreparation> {
    let gate = peer_guard(&source, host, transport)?;
    Ok(owner
        .begin_authorized(page, gate)?
        .prepare_saved_password_use(store, source, host, identity)
        .await?)
}
pub fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread().enable_all().max_blocking_threads(4).build()?.block_on(async{
    let registry=SessionRegistry::new();let documents=ProductDocuments::new(registry.clone());let window=registry.open_window()?;documents.install(window)?;documents.started(window)?;let nonce=documents.finished(window)?;documents.confirm(window,nonce)?;let page=documents.current(window,nonce)?;let owner=PageVaultOwner::new(registry.clone());
    let root=tempfile::tempdir()?;let dir=pulse_store::state_directory::prepare(&root.path().join("state"))?;let(store,worker)=StateStore::open(dir.join("state.db")).await?;let empty=store.load_vault_ciphertext(Arc::new(||true)).await?;let operation=owner.begin(page.clone())?;let(vault,creation)=operation.begin_creation(PasswordInput::new(b"fixed-upper-master".to_vec())?,empty.clone())?;operation.persist_creation(&store,empty,vault,creation.ready().await?).await?;let saved=store.load_vault_ciphertext(Arc::new(||true)).await?;let unlock=operation.begin_unlock(PasswordInput::new(b"fixed-upper-master".to_vec())?,saved.clone())?;operation.confirm_unlock(&store,saved,unlock.ready().await?).await?.accept()?;
    let value=SshPasswordIdentity::new("fixed","user",SecretRef::new(vault,Uuid::now_v7(),SecretKind::SshPassword,false)?)?;let meta=store.edit_ssh_identities(store.load_ssh_identities(Arc::new(||true)).await?,IdentityEdit::Create(value),Arc::new(||true)).await?;let identity=meta.records().next().ok_or_else(||anyhow::anyhow!("fixture.identity"))?.id();let source=store.load_identity_vault_source(Arc::new(||true)).await?;let _saved=owner.begin(page.clone())?.prepare_password_identity_write(source,identity)?.write(&store,SecretBytes::from_owned(b"fixed-not-authenticated".to_vec())?).await?;
    let key=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7;32]).into(),"")?;let listener=tokio::net::TcpListener::bind(("127.0.0.1",0)).await?;let address=listener.local_addr()?;let target=HostTarget::new("127.0.0.1",address.port())?;let auth=Arc::new(AtomicUsize::new(0));let copy=auth.clone();let config=Arc::new(server::Config{keys:vec![key.clone()],..Default::default()});let(tx,rx)=tokio::sync::oneshot::channel();let task=tokio::spawn(async move{let mut spy=Spy{auth:copy};let running=spy.run_on_socket(config,&listener);let _sent=tx.send(running.handle());running.await});let mut server=ServerGuard{stop:rx.await?,task:Some(task)};
    let hosts=store.edit_ssh_hosts(store.load_ssh_hosts().await?,HostEdit::Create(SshEndpoint::new("fixed-host","127.0.0.1",address.port(),"user")?),Arc::new(||true)).await?;let host=hosts.records()[0].id();let _trust=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,key.public_key())?).await?;
    let connect=|target:HostTarget|{let pin=HostPin::new(target.clone(),key.public_key().clone());async move{let stream=tokio::net::TcpStream::connect(address).await?;PinnedTransport::connect_stream(target,Some(vec![pin]),Some(vec![]),Arc::new(russh::client::Config::default()),stream).await.map_err(anyhow::Error::from)}};
    let connection=connect(target.clone()).await?;let source=store.load_ssh_target_source(Arc::new(||true)).await?;prepare(&owner,page.clone(),&store,source.clone(),host,identity,&connection).await?.validate(&store).await?;ensure!(auth.load(Ordering::SeqCst)==0,"fixture.auth_not_zero");
    let other=connect(HostTarget::new("other.example",address.port())?).await?;ensure!(peer_guard(&source,host,&other).is_err(),"fixture.foreign_target");drop(other);
    let prepared=prepare(&owner,page.clone(),&store,source.clone(),host,identity,&connection).await?;let _changed=store.edit_ssh_hosts(source.hosts().clone(),HostEdit::Update(host,SshEndpoint::new("changed-host","127.0.0.1",address.port(),"user")?),Arc::new(||true)).await?;ensure!(prepared.validate(&store).await.is_err(),"fixture.changed_source");
    let source=store.load_ssh_target_source(Arc::new(||true)).await?;let prepared=prepare(&owner,page.clone(),&store,source.clone(),host,identity,&connection).await?;drop(connection);ensure!(prepared.validate(&store).await.is_err(),"fixture.peer_dropped");
    let connection=connect(target.clone()).await?;let prepared=prepare(&owner,page.clone(),&store,source.clone(),host,identity,&connection).await?;documents.started(window)?;owner.retire_page()?;ensure!(prepared.validate(&store).await.is_err(),"fixture.retired_page");
    let bad=russh::keys::PrivateKey::new(russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[8;32]).into(),"")?;let _trust=store.edit_ssh_trust(store.load_ssh_trust().await?,pin_edit(&target,bad.public_key())?).await?;let source=store.load_ssh_target_source(Arc::new(||true)).await?;ensure!(peer_guard(&source,host,&connection).is_err(),"fixture.changed_pin");ensure!(auth.load(Ordering::SeqCst)==0,"fixture.auth_not_zero");
    drop(connection);server.stop.shutdown("fixed end".into());if let Some(task)=server.task.take(){tokio::time::timeout(std::time::Duration::from_secs(2),task).await???;}owner.close()?;drop(store);worker.join().await?;registry.shutdown()?;
    let output=crate::root()?.join("target/probes/ssh-target-preparation.json");std::fs::create_dir_all(output.parent().ok_or_else(||anyhow::anyhow!("fixture.output"))?)?;std::fs::write(output,serde_json::to_vec_pretty(&serde_json::json!({"schemaVersion":1,"preconditionsConfirmed":true,"wrongTargetRejected":true,"changedSourceRejected":true,"peerDropRejected":true,"pageRetirementRejected":true,"changedPinRejected":true,"authenticationRequests":0,"plaintextReturned":false,"credentialUsePermission":false,"productSshOpened":false}))?)?;
    println!("ssh.target_source_page_epoch_peer_preconditions_confirmed auth=0 credential_use=false");Ok(())
})
}
