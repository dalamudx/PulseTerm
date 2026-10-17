//! Trusted sink behavior tests, not real SSH authentication acceptance.
struct CreditExit(Option<tokio::sync::oneshot::Sender<()>>);
impl Drop for CreditExit {
    fn drop(&mut self) {
        if let Some(exit) = self.0.take() {
            let _ = exit.send(());
        }
    }
}
use super::*;
use crate::vault_owner::{
    PrivateKeyTargetSink, PrivateKeyTargetUseIntent, PrivateKeyTargetUseTask,
};
use std::{future::Future, pin::Pin};
#[derive(Default)]
struct Seen {
    authentication: AtomicUsize,
    drain: AtomicUsize,
    roles: AtomicBool,
}
struct Sink {
    host: Uuid,
    identity: Uuid,
    peer: bool,
    pass: bool,
    seen: Arc<Seen>,
    guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    auth_entered: Option<tokio::sync::oneshot::Sender<()>>,
    drain_entered: Option<tokio::sync::oneshot::Sender<()>>,
    native_exit: Option<tokio::sync::oneshot::Receiver<()>>,
    wait_auth: bool,
    decoded: Option<tokio::sync::oneshot::Sender<()>>,
    decode_wait: Option<tokio::sync::oneshot::Receiver<()>>,
}
impl Sink {
    fn new(f: &Fixture, seen: Arc<Seen>) -> Self {
        Self {
            host: f.host,
            identity: f.identity,
            peer: true,
            pass: false,
            seen,
            guard: None,
            auth_entered: None,
            drain_entered: None,
            native_exit: None,
            wait_auth: false,
            decoded: None,
            decode_wait: None,
        }
    }
}
impl PrivateKeyTargetSink for Sink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.peer
            && host == self.host
            && identity == self.identity
            && source.hosts().records().iter().any(|row| row.id() == host)
            && source
                .identities()
                .records()
                .any(|row| row.id() == identity)
    }
    fn retain_native_authorization(
        &mut self,
        guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<()> {
        self.guard = Some(guard);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        key: &'a [u8],
        pass: Option<&'a [u8]>,
        confirmation: &'a crate::vault_owner::PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if let Some(decoded) = self.decoded.take() {
                let _ = decoded.send(());
            }
            if let Some(wait) = self.decode_wait.take() {
                wait.await.map_err(|_| VaultOwnerError::Failed)?;
            }
            confirmation.confirm().await?;
            self.seen.authentication.fetch_add(1, Ordering::AcqRel);
            self.seen.roles.store(
                username == "user"
                    && key == b"fixed-role-bytes"
                    && if self.pass {
                        pass == Some(b"fixed-role-bytes".as_slice())
                    } else {
                        pass.is_none()
                    },
                Ordering::Release,
            );
            if let Some(entered) = self.auth_entered.take() {
                let _ = entered.send(());
            }
            if self.wait_auth {
                std::future::pending::<()>().await;
            }
            Ok(true)
        })
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.seen.drain.fetch_add(1, Ordering::AcqRel);
            if let Some(entered) = self.drain_entered.take() {
                let _ = entered.send(());
            }
            if let Some(exit) = self.native_exit.take() {
                exit.await.map_err(|_| VaultOwnerError::Failed)?;
            }
            Ok(())
        })
    }
}
async fn intent(
    f: &Fixture,
) -> std::result::Result<PrivateKeyTargetUseIntent, Box<dyn std::error::Error>> {
    Ok(f.owner
        .begin_private_key_target_action_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )?
        .prepare(&f.store, f.source().await?)
        .await?)
}
async fn start(
    f: &Fixture,
    sink: Sink,
) -> std::result::Result<PrivateKeyTargetUseTask, Box<dyn std::error::Error>> {
    Ok(intent(f).await?.start(f.store.clone(), sink)?)
}
#[test]
fn private_key_target_action_lends_both_roles_only_to_explicit_matching_native_sink() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let seen = Arc::new(Seen::default());
        assert!(start(&f, Sink::new(&f, seen.clone())).await?.wait().await?);
        assert!(seen.roles.load(Ordering::Acquire));
        assert_eq!(seen.authentication.load(Ordering::Acquire), 1);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        let pass = Uuid::now_v7();
        f.store
            .edit_ssh_key_identities(
                f.source().await?.identities().clone(),
                KeyIdentityEdit::Update(
                    f.identity,
                    SshPrivateKeyIdentity::new(
                        "fixed-key",
                        "user",
                        SecretRef::new(f.vault, f.key, SecretKind::SshPrivateKey, false)?,
                        Some(SecretRef::new(
                            f.vault,
                            pass,
                            SecretKind::SshKeyPassphrase,
                            false,
                        )?),
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        f.seal(pass, SecretKind::SshKeyPassphrase).await?;
        let seen = Arc::new(Seen::default());
        let mut sink = Sink::new(&f, seen.clone());
        sink.pass = true;
        assert!(start(&f, sink).await?.wait().await?);
        assert!(seen.roles.load(Ordering::Acquire));
        assert_eq!(seen.authentication.load(Ordering::Acquire), 1);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.finish().await
    })
}
#[test]
fn private_key_target_rejection_keeps_operation_until_native_exit_and_authenticates_zero_times()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let seen = Arc::new(Seen::default());
        let (entered, observed) = tokio::sync::oneshot::channel();
        let (exit, exited) = tokio::sync::oneshot::channel();
        let mut sink = Sink::new(&f, seen.clone());
        sink.peer = false;
        sink.drain_entered = Some(entered);
        sink.native_exit = Some(exited);
        let task = start(&f, sink).await?;
        tokio::time::timeout(std::time::Duration::from_secs(2), observed).await??;
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        exit.send(()).map_err(|_| "fixture.exit")?;
        assert!(task.wait().await.is_err());
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.prepare().await?.validate(&f.store).await?;
        f.finish().await
    })
}
#[test]
fn private_key_target_source_and_page_revocation_reject_before_authentication() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let prepared = intent(&f).await?;
        f.store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                HostEdit::Update(
                    f.host,
                    SshEndpoint::new("changed", "other.example", 22, "user")?,
                ),
                Arc::new(|| true),
            )
            .await?;
        let seen = Arc::new(Seen::default());
        assert!(
            prepared
                .start(f.store.clone(), Sink::new(&f, seen.clone()))?
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        let prepared = intent(&f).await?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        let seen = Arc::new(Seen::default());
        assert!(
            prepared
                .start(f.store.clone(), Sink::new(&f, seen.clone()))?
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.finish().await
    })
}
#[test]
fn private_key_target_cancellation_does_not_release_native_credit_or_retry_authentication()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let seen = Arc::new(Seen::default());
        let (auth, observed_auth) = tokio::sync::oneshot::channel();
        let (drain, observed_drain) = tokio::sync::oneshot::channel();
        let (exit, exited) = tokio::sync::oneshot::channel();
        let mut sink = Sink::new(&f, seen.clone());
        sink.auth_entered = Some(auth);
        sink.drain_entered = Some(drain);
        sink.native_exit = Some(exited);
        sink.wait_auth = true;
        let (credit_exit, observed_credit_exit) = tokio::sync::oneshot::channel();
        let credit = Arc::new(CreditExit(Some(credit_exit)));
        let action = f.owner.begin_private_key_target_action_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || {
                let _retained = &credit;
                true
            }),
        )?;
        let task = action
            .prepare(&f.store, f.source().await?)
            .await?
            .start(f.store.clone(), sink)?;
        tokio::time::timeout(std::time::Duration::from_secs(2), observed_auth).await??;
        drop(task);
        tokio::time::timeout(std::time::Duration::from_secs(2), observed_drain).await??;
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        assert_eq!(seen.authentication.load(Ordering::Acquire), 1);
        exit.send(()).map_err(|_| "fixture.exit")?;
        tokio::time::timeout(std::time::Duration::from_secs(2), observed_credit_exit).await??;
        let operation = f.owner.begin(f.page.scope.clone())?;
        drop(operation);
        assert_eq!(seen.authentication.load(Ordering::Acquire), 1);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.finish().await
    })
}

#[test]
fn private_key_target_late_source_confirmation_rejects_changes_during_native_decode() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let seen = Arc::new(Seen::default());
        let (decoded, observed_decode) = tokio::sync::oneshot::channel();
        let (release, decode_wait) = tokio::sync::oneshot::channel();
        let mut sink = Sink::new(&f, seen.clone());
        sink.decoded = Some(decoded);
        sink.decode_wait = Some(decode_wait);
        let task = start(&f, sink).await?;
        tokio::time::timeout(std::time::Duration::from_secs(2), observed_decode).await??;
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        f.store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                HostEdit::Update(
                    f.host,
                    SshEndpoint::new("changed-after-decode", "other.example", 22, "user")?,
                ),
                Arc::new(|| true),
            )
            .await?;
        release.send(()).map_err(|_| "fixture.decode_release")?;
        assert!(task.wait().await.is_err());
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.finish().await
    })
}

#[test]
fn private_key_target_action_rejects_same_dek_reunlock_after_issuance() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let action = f.owner.begin_private_key_target_action_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )?;
        let scope = f
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        scope.lock()?;
        let saved = f.store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = pulse_store::vault_unlock::begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-private-preparation-master".to_vec())?,
            saved.envelope().ok_or("fixture.envelope")?.clone(),
        )?;
        let mut ready = request.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        let seen = Arc::new(Seen::default());
        assert!(start(&f, Sink::new(&f, seen.clone())).await?.wait().await?);
        assert_eq!(seen.authentication.load(Ordering::Acquire), 1);
        f.finish().await
    })
}
#[test]
fn private_key_target_rejects_wrong_dek_ciphertext_before_authentication_and_drains() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let wrong = pulse_store::vault_crypto::VaultCipher::generate()?;
        let record = CredentialRecord::seal(
            f.vault,
            f.key,
            SecretKind::SshPrivateKey.as_str(),
            &wrong,
            b"fixed-wrong-dek",
        )?;
        f.store
            .edit_vault_ciphertext(
                f.source().await?.vault().clone(),
                VaultEdit::Replace(record),
                Arc::new(|| true),
            )
            .await?;
        let seen = Arc::new(Seen::default());
        assert!(
            start(&f, Sink::new(&f, seen.clone()))
                .await?
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.authentication.load(Ordering::Acquire), 0);
        assert_eq!(seen.drain.load(Ordering::Acquire), 1);
        f.finish().await
    })
}
