//! Actual Vault ciphertext and actor/native joins; not a real SSH peer acceptance substitute.
use super::*;
#[path = "vault_key_session_gesture_tests.rs"]
mod gesture;
use crate::{
    protocol_session_owner::{ProtocolSessionError, ProtocolSessionReservation},
    vault_owner::{
        AuthenticatedProtocolParts, PrivateKeySessionIntent, PrivateKeySessionSink,
        PrivateKeySourceConfirmation, PrivateKeyTargetSink,
    },
};
use pulse_core::TermSize;
use pulse_term::session::{self, ChannelTransport};
use std::{future::Future, pin::Pin};
#[derive(Default)]
struct Seen {
    auth: AtomicUsize,
    transferred: AtomicUsize,
    setup: tokio::sync::Notify,
    draining: tokio::sync::Notify,
    actor_done: tokio::sync::Notify,
}
struct Sink {
    host: Uuid,
    identity: Uuid,
    seen: Arc<Seen>,
    guard: Option<Arc<dyn Fn() -> bool + Send + Sync>>,
    pending: bool,
    lock: Option<PageVaultOwner>,
    cleanup: Option<tokio::sync::oneshot::Receiver<()>>,
    native: Option<tokio::sync::oneshot::Receiver<()>>,
}
impl Sink {
    fn new(f: &Fixture) -> Self {
        Self {
            host: f.host,
            identity: f.identity,
            seen: Arc::new(Seen::default()),
            guard: None,
            pending: false,
            lock: None,
            cleanup: None,
            native: None,
        }
    }
}
impl PrivateKeyTargetSink for Sink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.host == host
            && self.identity == identity
            && source.identities().records().any(|r| r.id() == identity)
    }
    fn retain_native_authorization(
        &mut self,
        g: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::result::Result<(), VaultOwnerError> {
        self.guard = Some(g);
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        key: &'a [u8],
        pass: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        Box::pin(async move {
            confirmation.confirm().await?;
            if user != "user"
                || key != b"fixed-role-bytes"
                || pass.is_some()
                || !self.guard.as_ref().is_some_and(|g| g())
            {
                return Err(VaultOwnerError::Denied);
            }
            self.seen.auth.fetch_add(1, Ordering::SeqCst);
            Ok(true)
        })
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            self.seen.draining.notify_one();
            if let Some(c) = self.cleanup.take() {
                c.await.map_err(|_| VaultOwnerError::Failed)?;
            }
            Ok(())
        })
    }
}
impl PrivateKeySessionSink for Sink {
    fn prepare_authenticated_terminal(
        &mut self,
        g: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            self.seen.setup.notify_one();
            if let Some(owner) = self.lock.take() {
                owner.lock()?;
            }
            if self.pending {
                std::future::pending::<()>().await;
            }
            if g() {
                Ok(())
            } else {
                Err(VaultOwnerError::Denied)
            }
        })
    }
    fn take_authenticated_root_parts(
        &mut self,
    ) -> std::result::Result<AuthenticatedProtocolParts, VaultOwnerError> {
        self.seen.transferred.fetch_add(1, Ordering::SeqCst);
        self.guard.take();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = session::spawn_session(
            TermSize::new(32, 8).map_err(|_| VaultOwnerError::Failed)?,
            64,
            transport,
        );
        let mut state = handle.state.clone();
        let seen = self.seen.clone();
        let native = self.native.take();
        Ok(AuthenticatedProtocolParts::new(handle, async move {
            state
                .wait_for(|s| s.lifecycle != session::Lifecycle::Running)
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            drop(driver);
            task.join()
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            seen.actor_done.notify_one();
            if let Some(native) = native {
                native
                    .await
                    .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            }
            Ok(())
        }))
    }
}
async fn intent(
    f: &Fixture,
) -> std::result::Result<PrivateKeySessionIntent, Box<dyn std::error::Error>> {
    Ok(f.owner
        .begin_private_key_session_action_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )?
        .prepare(&f.store, f.source().await?)
        .await?)
}
async fn signal(n: &tokio::sync::Notify) -> TestResult {
    tokio::time::timeout(std::time::Duration::from_secs(2), n.notified()).await?;
    Ok(())
}
#[test]
fn private_key_session_root_survives_navigation_and_retains_slot_until_native_exit() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let mut sink = Sink::new(&f);
        let seen = sink.seen.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        sink.native = Some(rx);
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        let owned = intent(&f)
            .await?
            .start(f.store.clone(), sink, started)
            .wait()
            .await?;
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert_eq!(seen.transferred.load(Ordering::SeqCst), 1);
        assert!(owned.is_managed_by(&f.page.registry));
        drop(f.owner.begin(f.page.scope.clone())?);
        let _new_page = f.page.registry.activate_launch_page(f.page.window)?;
        assert_eq!(
            owned.handle().state.borrow().lifecycle,
            session::Lifecycle::Running
        );
        let id = owned.id();
        let mut cleanup = owned.observe_cleanup();
        drop(owned.cancel());
        signal(&seen.actor_done).await?;
        assert_eq!(
            *cleanup.borrow(),
            crate::local_session_owner::CleanupState::Closing
        );
        assert!(f.page.registry.state(f.page.window, id).is_ok());
        tx.send(()).map_err(|_| "fixture.release")?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            cleanup.wait_for(|s| *s == crate::local_session_owner::CleanupState::Reaped),
        )
        .await??;
        f.finish().await
    })
}
#[test]
fn private_key_session_foreign_reservation_stale_source_page_and_locked_epoch_reject() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let i = intent(&f).await?;
        let w = f.page.registry.open_window()?;
        let other = f.page.registry.activate_launch_page(w)?;
        let sink = Sink::new(&f);
        let seen = sink.seen.clone();
        let started = ProtocolSessionReservation::reserve(&f.page.registry, &other)?.begin()?;
        assert!(
            i.start(f.store.clone(), sink, started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.auth.load(Ordering::SeqCst), 0);
        let i = intent(&f).await?;
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        f.store
            .edit_ssh_hosts(
                f.store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new("other", "other.example", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        let sink = Sink::new(&f);
        let seen = sink.seen.clone();
        assert!(
            i.start(f.store.clone(), sink, started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.auth.load(Ordering::SeqCst), 0);
        let i = intent(&f).await?;
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        let mut sink = Sink::new(&f);
        let seen = sink.seen.clone();
        sink.lock = Some(f.owner.clone());
        assert!(
            i.start(f.store.clone(), sink, started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert_eq!(seen.transferred.load(Ordering::SeqCst), 0);
        f.finish().await
    })
}
#[test]
fn private_key_session_cancel_setup_holds_operation_until_actual_cleanup() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let mut sink = Sink::new(&f);
        sink.pending = true;
        let seen = sink.seen.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        sink.cleanup = Some(rx);
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        let task = intent(&f).await?.start(f.store.clone(), sink, started);
        signal(&seen.setup).await?;
        drop(task);
        signal(&seen.draining).await?;
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        assert_eq!(seen.transferred.load(Ordering::SeqCst), 0);
        tx.send(()).map_err(|_| "fixture.release")?;
        f.page.registry.seal_native_workers().join().await?;
        drop(f.owner.begin(f.page.scope.clone())?);
        f.finish().await
    })
}
#[test]
fn private_key_session_lost_owner_receipt_still_joins_native_resources() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let mut sink = Sink::new(&f);
        let seen = sink.seen.clone();
        let (tx, rx) = tokio::sync::oneshot::channel();
        sink.native = Some(rx);
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        let task = intent(&f).await?.start(f.store.clone(), sink, started);
        signal(&seen.draining).await?;
        assert_eq!(seen.transferred.load(Ordering::SeqCst), 1);
        drop(task);
        signal(&seen.actor_done).await?;
        tx.send(()).map_err(|_| "fixture.release")?;
        f.page.registry.seal_native_workers().join().await?;
        f.finish().await
    })
}
