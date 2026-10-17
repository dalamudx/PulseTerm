use super::*;
#[path = "vault_target_session_tests.rs"]
mod session_tests;
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_endpoint::SshEndpoint,
    ssh_identity::SshPasswordIdentity,
};
use pulse_store::{
    ssh_hosts::HostEdit, ssh_identities::IdentityEdit, ssh_target_source::SshTargetSource,
    vault_crypto::SecretBytes,
};
use std::{future::Future, pin::Pin, sync::atomic::AtomicUsize};
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
struct Seen {
    auth: AtomicUsize,
    exact: AtomicBool,
    started: tokio::sync::Notify,
    draining: tokio::sync::Notify,
}
impl Seen {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            auth: AtomicUsize::new(0),
            exact: AtomicBool::new(false),
            started: tokio::sync::Notify::new(),
            draining: tokio::sync::Notify::new(),
        })
    }
}
struct Sink {
    selected: (Uuid, Uuid),
    seen: Arc<Seen>,
    guard: Option<Guard>,
    pending: bool,
    release: Option<tokio::sync::oneshot::Receiver<()>>,
}
impl PasswordTargetSink for Sink {
    fn matches_target(&self, _source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.selected == (host, identity)
    }
    fn retain_native_authorization(
        &mut self,
        guard: Guard,
    ) -> std::result::Result<(), VaultOwnerError> {
        self.guard = Some(guard);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        Box::pin(async move {
            self.seen.auth.fetch_add(1, Ordering::SeqCst);
            self.seen.exact.store(
                user == "user" && password == "fixed-target-secret",
                Ordering::Release,
            );
            self.seen.started.notify_one();
            if self.pending {
                std::future::pending::<()>().await;
            }
            Ok(true)
        })
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            self.seen.draining.notify_one();
            if let Some(release) = self.release.take() {
                release.await.map_err(|_| VaultOwnerError::Failed)?;
            }
            self.guard.take();
            Ok(())
        })
    }
}
fn sink(
    selected: (Uuid, Uuid),
    pending: bool,
    blocked: bool,
) -> (Sink, Arc<Seen>, tokio::sync::oneshot::Sender<()>) {
    let seen = Seen::new();
    let (tx, rx) = tokio::sync::oneshot::channel();
    (
        Sink {
            selected,
            seen: seen.clone(),
            guard: None,
            pending,
            release: blocked.then_some(rx),
        },
        seen,
        tx,
    )
}
async fn await_signal(signal: &tokio::sync::Notify) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(2), signal.notified()).await?;
    Ok(())
}
#[test]
fn actual_ciphertext_explicit_action_exact_borrow_source_cancel_and_late_drain_never_return_plaintext_or_upgrade_control()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let op = owner.begin(p.scope.clone())?;
        let (vault, request) = op.begin_creation(
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            empty.clone(),
        )?;
        op.persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let source = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        // Creation retains the same native operation; Rust shadowing would not drop it.
        let request = op.begin_unlock(
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            source.clone(),
        )?;
        op.confirm_unlock(&store, source, request.ready().await?)
            .await?
            .accept()?;
        let identity = store
            .edit_ssh_identities(
                store.load_ssh_identities(Arc::new(|| true)).await?,
                IdentityEdit::Create(SshPasswordIdentity::new(
                    "fixed",
                    "user",
                    SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
                )?),
                Arc::new(|| true),
            )
            .await?;
        let id = identity.records().next().ok_or("fixture.id")?.id();
        let hosts = store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new("fixed", "fixed.example", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        let host = hosts.records()[0].id();
        let selected = (host, id);
        owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(
                store.load_identity_vault_source(Arc::new(|| true)).await?,
                id,
            )?
            .write(
                &store,
                SecretBytes::from_owned(b"fixed-target-secret".to_vec())?,
            )
            .await?;
        let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
        // Generic read-only preparation only validates. Compile-fail doc prevents conversion into an action intent.
        owner
            .begin(p.scope.clone())?
            .prepare_saved_password_use(&store, source.clone(), host, id)
            .await?
            .validate(&store)
            .await?;
        // GUI consumes an original epoch, then the same DEK is independently re-authenticated
        // before native prepare. No Token cancellation: only pinning can reject this upgrade.
        let gesture = owner.begin_password_session_gesture_authorized(
            p.scope.clone(),
            host,
            id,
            Arc::new(|| true),
        )?;
        let action = gesture.begin_session_authorized(&owner, &p.scope, Arc::new(|| true))?;
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let original = scope.available_epoch()?;
        scope.lock()?;
        let unlock = pulse_store::vault_unlock::begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            source
                .identity_vault()
                .vault()
                .envelope()
                .ok_or("fixture.envelope")?
                .clone(),
        )?;
        let mut ready = unlock.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert!(!original.is_original_current()?);
        assert!(scope.available_epoch()?.is_original_current()?);
        assert!(action.prepare(&store, source.clone()).await.is_err());
        drop(scope);
        let intent = owner
            .begin_password_target_action_authorized(p.scope.clone(), host, id, Arc::new(|| true))?
            .prepare(&store, source.clone())
            .await?;
        let (receiver, seen, _tx) = sink(selected, false, false);
        assert!(intent.start(store.clone(), receiver)?.wait().await?);
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert!(seen.exact.load(Ordering::Acquire));
        assert_eq!(
            owner
                .begin(p.scope.clone())?
                .status(source.identity_vault().vault().clone())?,
            Some(KeyStatus::Available)
        );
        let intent = owner
            .begin_password_target_action_authorized(p.scope.clone(), host, id, Arc::new(|| true))?
            .prepare(&store, source.clone())
            .await?;
        let (receiver, seen, _tx) = sink((Uuid::now_v7(), id), false, false);
        assert!(intent.start(store.clone(), receiver).is_err());
        assert_eq!(seen.auth.load(Ordering::SeqCst), 0);
        // Same-DEK direct reauthentication does not cancel the root Token, but the original weak key epoch stays stale.
        let intent = owner
            .begin_password_target_action_authorized(p.scope.clone(), host, id, Arc::new(|| true))?
            .prepare(&store, source.clone())
            .await?;
        let scope = owner.inner.guard()?.scope.clone().ok_or("fixture.scope")?;
        let original = scope.available_epoch()?;
        assert!(original.is_original_current()?);
        scope.lock()?;
        let unlock = pulse_store::vault_unlock::begin_validation_unlock(
            &scope,
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            source
                .identity_vault()
                .vault()
                .envelope()
                .ok_or("fixture.envelope")?
                .clone(),
        )?;
        let mut ready = unlock.ready().await?;
        ready.confirm(&scope)?;
        drop(ready);
        assert!(!original.is_original_current()?);
        assert!(scope.available_epoch()?.is_original_current()?);
        let (receiver, seen, _tx) = sink(selected, false, false);
        assert!(intent.start(store.clone(), receiver).is_err());
        assert_eq!(seen.auth.load(Ordering::SeqCst), 0);
        drop(scope);
        // Waiter abandonment cancels the original authority, but detached supervisor drains before releasing its slot.
        let intent = owner
            .begin_password_target_action_authorized(p.scope.clone(), host, id, Arc::new(|| true))?
            .prepare(&store, source.clone())
            .await?;
        let (receiver, seen, release) = sink(selected, true, true);
        let task = intent.start(store.clone(), receiver)?;
        await_signal(&seen.started).await?;
        drop(task);
        await_signal(&seen.draining).await?;
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        release.send(()).map_err(|_| "fixture.release")?;
        let operation = tokio::time::timeout(std::time::Duration::from_secs(2), async {
            loop {
                match owner.begin(p.scope.clone()) {
                    Ok(operation) => break Ok::<_, VaultOwnerError>(operation),
                    Err(VaultOwnerError::Busy) => tokio::task::yield_now().await,
                    Err(e) => break Err(e),
                }
            }
        })
        .await??;
        drop(operation);
        // Success before blocked drain must not become a current receipt after manual lock while cleanup remains active.
        let intent = owner
            .begin_password_target_action_authorized(p.scope.clone(), host, id, Arc::new(|| true))?
            .prepare(&store, source.clone())
            .await?;
        let (receiver, seen, release) = sink(selected, false, true);
        let task = intent.start(store.clone(), receiver)?;
        await_signal(&seen.draining).await?;
        owner.lock()?;
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        release.send(()).map_err(|_| "fixture.release")?;
        assert!(task.wait().await.is_err());
        assert_ne!(
            owner
                .begin(p.scope.clone())?
                .status(source.identity_vault().vault().clone())?,
            Some(KeyStatus::Available)
        );
        owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
