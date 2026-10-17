//! Isolated fixed loopback probe: authenticates ciphertext through the actual candidate adapter.
use crate::ssh_private_key_adapter as adapter;
#[path = "ssh_private_key_session_probe.rs"]
mod session;
use anyhow::{Result, ensure};
use pulse_app::{
    product_document::ProductDocuments,
    registry::{PageLaunchScope, SessionRegistry},
    vault_owner::{PageVaultOwner, PrivateKeyTargetUseTask},
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_key_identity::SshPrivateKeyIdentity,
};
use pulse_ssh::{HostPin, HostTarget, managed_store::pin_edit, peer::PinnedTransport};
use pulse_store::{
    StateStore, ssh_hosts::HostEdit, ssh_key_identities::KeyIdentityEdit,
    vault_crypto::VaultCipher, vault_database::VaultEdit, vault_record::CredentialRecord,
    vault_unlock::PasswordInput, vault_wrap::create_validation_vault,
};
use russh::{
    keys::{PrivateKey, PublicKey, ssh_key::LineEnding},
    server::{self, Server},
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
const MASTER: &[u8] = b"fixed-loopback-private-master";
const PASS: &[u8] = b"fixed-loopback-private-pass";
#[derive(Default)]
struct Counts {
    offered: AtomicUsize,
    signed: AtomicUsize,
    other_auth: AtomicUsize,
    closed: AtomicUsize,
    exit: tokio::sync::Notify,
}
struct Spy {
    counts: Arc<Counts>,
    allowed: PublicKey,
    session: bool,
}
impl Drop for Spy {
    fn drop(&mut self) {
        if self.session {
            self.counts.closed.fetch_add(1, Ordering::SeqCst);
            self.counts.exit.notify_one();
        }
    }
}
impl Server for Spy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            counts: self.counts.clone(),
            allowed: self.allowed.clone(),
            session: true,
        }
    }
}
impl server::Handler for Spy {
    type Error = russh::Error;
    async fn auth_none(&mut self, _: &str) -> std::result::Result<server::Auth, Self::Error> {
        self.counts.other_auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_password(
        &mut self,
        _: &str,
        _: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.counts.other_auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.counts.offered.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "user" && key == &self.allowed {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.counts.signed.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "user" && key == &self.allowed {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
}
struct ServerGuard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.shutdown("fixed probe end".into());
        if let Some(task) = self.task.take() {
            task.abort();
        }
    }
}
async fn wait_closed(counts: &Counts, expected: usize) -> Result<()> {
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        loop {
            let receipt = counts.exit.notified();
            if counts.closed.load(Ordering::SeqCst) >= expected {
                break;
            }
            receipt.await;
        }
    })
    .await
    .map_err(|_| anyhow::anyhow!("fixture.remote_exit_timeout"))?;
    Ok(())
}
fn wire_string<'a>(wire: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let length_end = cursor
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    let length = usize::try_from(u32::from_be_bytes(
        wire.get(*cursor..length_end)
            .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?
            .try_into()?,
    ))?;
    let end = length_end
        .checked_add(length)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    let field = wire
        .get(length_end..end)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    *cursor = end;
    Ok(field)
}
// Alter a real, valid encrypted OpenSSH header without performing the attacker-requested KDF.
fn malicious_rounds(encrypted: &PrivateKey, rounds: u32) -> Result<zeroize::Zeroizing<String>> {
    let mut wire = encrypted.to_bytes()?;
    let magic = b"openssh-key-v1\0";
    ensure!(wire.starts_with(magic), "fixture.magic");
    let mut cursor = magic.len();
    let _cipher = wire_string(&wire, &mut cursor)?;
    ensure!(wire_string(&wire, &mut cursor)? == b"bcrypt", "fixture.kdf");
    let options_start = cursor
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    let options = wire_string(&wire, &mut cursor)?;
    let mut offset = 0;
    let _salt = wire_string(options, &mut offset)?;
    ensure!(
        offset.checked_add(4) == Some(options.len()),
        "fixture.kdf_shape"
    );
    let start = options_start
        .checked_add(offset)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    let end = start
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?;
    wire.get_mut(start..end)
        .ok_or_else(|| anyhow::anyhow!("fixture.wire"))?
        .copy_from_slice(&rounds.to_be_bytes());
    Ok(PrivateKey::from_bytes(&wire)?.to_openssh(LineEnding::LF)?)
}
fn seal(
    cipher: &mut VaultCipher,
    vault: Uuid,
    id: Uuid,
    kind: SecretKind,
    bytes: &[u8],
) -> Result<CredentialRecord> {
    Ok(CredentialRecord::seal(
        vault,
        id,
        kind.as_str(),
        cipher,
        bytes,
    )?)
}
async fn launch(
    owner: &PageVaultOwner,
    page: Arc<PageLaunchScope>,
    store: &StateStore,
    host: Uuid,
    identity: Uuid,
    transport: PinnedTransport,
) -> Result<PrivateKeyTargetUseTask> {
    let source = store.load_ssh_key_target_source(Arc::new(|| true)).await?;
    let guard = adapter::peer_guard(&source, host, &transport)?;
    let intent = owner
        .begin_private_key_target_action_authorized(page, host, identity, guard)?
        .prepare(store, source)
        .await?;
    Ok(intent.start(
        store.clone(),
        adapter::Sink::new(transport, (host, identity))?,
    )?)
}
pub fn run_session() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(4)
        .build()?
        .block_on(session::run_async())
}
pub fn run_algorithms() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(4)
        .build()?
        .block_on(session::run_algorithms())
}
pub fn run() -> Result<()> {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(4)
        .build()?
        .block_on(run_async())
}
async fn run_async() -> Result<()> {
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
    let fixture: Result<()> = async {
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
        let operation = owner.begin(page.clone())?;
        let unlock = operation.begin_unlock(PasswordInput::new(MASTER.to_vec())?, saved.clone())?;
        operation
            .confirm_unlock(&store, saved, unlock.ready().await?)
            .await?
            .accept()?;
        let user_key = PrivateKey::new(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
            "",
        )?;
        let pem = user_key.to_openssh(LineEnding::LF)?;
        let key_id = Uuid::now_v7();
        let pass_id = Uuid::now_v7();
        let plain_record = seal(
            &mut cipher,
            vault,
            key_id,
            SecretKind::SshPrivateKey,
            pem.as_bytes(),
        )?;
        let encrypted = user_key.encrypt(&mut rand::rng(), PASS)?;
        let encrypted_pem = encrypted.to_openssh(LineEnding::LF)?;
        let encrypted_record = seal(
            &mut cipher,
            vault,
            key_id,
            SecretKind::SshPrivateKey,
            encrypted_pem.as_bytes(),
        )?;
        let pass_record = seal(
            &mut cipher,
            vault,
            pass_id,
            SecretKind::SshKeyPassphrase,
            PASS,
        )?;
        let wrong_pass = seal(
            &mut cipher,
            vault,
            pass_id,
            SecretKind::SshKeyPassphrase,
            b"wrong-fixed-pass",
        )?;
        store
            .edit_vault_ciphertext(
                store.load_vault_ciphertext(Arc::new(|| true)).await?,
                VaultEdit::Create(plain_record),
                Arc::new(|| true),
            )
            .await?;
        let identities = store
            .edit_ssh_key_identities(
                store.load_ssh_key_identities(Arc::new(|| true)).await?,
                KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
                    "fixed-key",
                    "user",
                    SecretRef::new(vault, key_id, SecretKind::SshPrivateKey, false)?,
                    None,
                )?),
                Arc::new(|| true),
            )
            .await?;
        let identity = identities
            .records()
            .next()
            .ok_or_else(|| anyhow::anyhow!("fixture.identity"))?
            .id();
        let server_key = PrivateKey::new(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[7; 32]).into(),
            "",
        )?;
        let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
        let address = listener.local_addr()?;
        let target = HostTarget::new("127.0.0.1", address.port())?;
        let counts = Arc::new(Counts::default());
        let copy = counts.clone();
        let allowed = user_key.public_key().clone();
        let config = Arc::new(server::Config {
            keys: vec![server_key.clone()],
            ..Default::default()
        });
        let (tx, rx) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut spy = Spy {
                counts: copy,
                allowed,
                session: false,
            };
            let running = spy.run_on_socket(config, &listener);
            let _sent = tx.send(running.handle());
            running.await
        });
        let mut server = ServerGuard {
            stop: rx.await?,
            task: Some(task),
        };
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
                    pin_edit(&target, server_key.public_key())?,
                )
                .await?;
            let connect = || {
                let target = target.clone();
                let pin = HostPin::new(target.clone(), server_key.public_key().clone());
                async move {
                    PinnedTransport::connect_stream(
                        target,
                        Some(vec![pin]),
                        Some(vec![]),
                        Arc::new(russh::client::Config::default()),
                        tokio::net::TcpStream::connect(address).await?,
                    )
                    .await
                    .map_err(anyhow::Error::from)
                }
            };
            ensure!(
                launch(
                    &owner,
                    page.clone(),
                    &store,
                    host,
                    identity,
                    connect().await?
                )
                .await?
                .wait()
                .await?,
                "fixture.plain_auth"
            );
            wait_closed(&counts, 1).await?;
            ensure!(
                counts.signed.load(Ordering::SeqCst) == 1,
                "fixture.plain_signature"
            );
            store
                .edit_vault_ciphertext(
                    store.load_vault_ciphertext(Arc::new(|| true)).await?,
                    VaultEdit::Replace(encrypted_record),
                    Arc::new(|| true),
                )
                .await?;
            store
                .edit_vault_ciphertext(
                    store.load_vault_ciphertext(Arc::new(|| true)).await?,
                    VaultEdit::Create(pass_record),
                    Arc::new(|| true),
                )
                .await?;
            store
                .edit_ssh_key_identities(
                    store.load_ssh_key_identities(Arc::new(|| true)).await?,
                    KeyIdentityEdit::Update(
                        identity,
                        SshPrivateKeyIdentity::new(
                            "fixed-key",
                            "user",
                            SecretRef::new(vault, key_id, SecretKind::SshPrivateKey, false)?,
                            Some(SecretRef::new(
                                vault,
                                pass_id,
                                SecretKind::SshKeyPassphrase,
                                false,
                            )?),
                        )?,
                    ),
                    Arc::new(|| true),
                )
                .await?;
            ensure!(
                launch(
                    &owner,
                    page.clone(),
                    &store,
                    host,
                    identity,
                    connect().await?
                )
                .await?
                .wait()
                .await?,
                "fixture.encrypted_auth"
            );
            wait_closed(&counts, 2).await?;
            ensure!(
                counts.signed.load(Ordering::SeqCst) == 2,
                "fixture.encrypted_signature"
            );
            store
                .edit_vault_ciphertext(
                    store.load_vault_ciphertext(Arc::new(|| true)).await?,
                    VaultEdit::Replace(wrong_pass),
                    Arc::new(|| true),
                )
                .await?;
            ensure!(
                launch(
                    &owner,
                    page.clone(),
                    &store,
                    host,
                    identity,
                    connect().await?
                )
                .await?
                .wait()
                .await
                .is_err(),
                "fixture.wrong_pass_allowed"
            );
            wait_closed(&counts, 3).await?;
            ensure!(
                counts.signed.load(Ordering::SeqCst) == 2
                    && counts.offered.load(Ordering::SeqCst) == 2,
                "fixture.wrong_pass_auth_sent"
            );
            for rounds in [0, 65, u32::MAX] {
                let malicious = malicious_rounds(&encrypted, rounds)?;
                ensure!(
                    adapter::decode_bounded(malicious.as_bytes(), Some(PASS)).is_err(),
                    "fixture.unbounded_kdf"
                );
            }
            ensure!(
                adapter::decode_bounded(&vec![b'x'; 65_537], None).is_err(),
                "fixture.key_budget"
            );
            ensure!(
                adapter::decode_bounded(pem.as_bytes(), Some(&vec![b'x'; 4097])).is_err(),
                "fixture.pass_budget"
            );
            ensure!(
                counts.other_auth.load(Ordering::SeqCst) == 0,
                "fixture.auth_fallback"
            );
            Ok(())
        }
        .await;
        server.stop.shutdown("fixed probe end".into());
        let server_result: Result<()> = async {
            if let Some(mut task) = server.task.take() {
                match tokio::time::timeout(std::time::Duration::from_secs(2), &mut task).await {
                    Ok(result) => {
                        result.map_err(|_| anyhow::anyhow!("fixture.server_join"))??;
                    }
                    Err(_) => {
                        task.abort();
                        let _terminal = task.await;
                        return Err(anyhow::anyhow!("fixture.server_exit"));
                    }
                }
            }
            Ok(())
        }
        .await;
        server_result?;
        result
    }
    .await;
    let owner_result = owner.close();
    drop(store);
    let worker_result = worker.join().await;
    let registry_result = registry.shutdown();
    owner_result?;
    worker_result?;
    registry_result?;
    fixture?;
    let output = crate::root()?.join("target/probes/ssh-private-key-use.json");
    std::fs::create_dir_all(
        output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("fixture.output"))?,
    )?;
    std::fs::write(
        output,
        serde_json::to_vec_pretty(
            &serde_json::json!({"schemaVersion": 1, "signedAuthentications": 2, "unencryptedKey": true, "encryptedKey": true, "wrongPassphraseNoAuthentication": true, "localAndRemoteExitConfirmed": true, "passwordOrNoneRequests": 0, "plaintextReturned": false, "productEntryOpened": false}),
        )?,
    )?;
    println!(
        "ssh.private_key_ciphertext_ed25519_plain_and_encrypted_signed_auth_wrong_pass_zero_auth_and_native_exit_confirmed"
    );
    Ok(())
}
