use super::*;
use crate::protocol_session_owner::{ProtocolSessionError, ProtocolSessionReservation};
use pulse_core::TermSize;
use pulse_term::session::{self, ChannelTransport};

struct SessionSink {
    auth: Sink,
    lock_after_auth: Option<PageVaultOwner>,
    setup_pending: bool,
    setup_started: Arc<tokio::sync::Notify>,
    transferred: Arc<AtomicUsize>,
    native_release: Option<tokio::sync::oneshot::Receiver<()>>,
    appearance: pulse_store::profile_editor::ProfileAppearance,
    actor_done: Arc<tokio::sync::Notify>,
}
impl PasswordTargetSink for SessionSink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.auth.matches_target(source, host, identity)
    }
    fn retain_native_authorization(
        &mut self,
        guard: Guard,
    ) -> std::result::Result<(), VaultOwnerError> {
        self.auth.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        self.auth.authenticate(user, password)
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        self.auth.close_and_drain()
    }
}
impl PasswordSessionSink for SessionSink {
    fn prepare_authenticated_terminal(
        &mut self,
        authorization: Guard,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            self.setup_started.notify_one();
            if let Some(owner) = self.lock_after_auth.take() {
                owner.lock()?;
            }
            if self.setup_pending {
                std::future::pending::<()>().await;
            }
            if authorization() {
                Ok(())
            } else {
                Err(VaultOwnerError::Denied)
            }
        })
    }
    fn take_authenticated_root_parts(
        &mut self,
    ) -> std::result::Result<AuthenticatedProtocolParts, VaultOwnerError> {
        self.transferred.fetch_add(1, Ordering::SeqCst);
        // Auth/setup completed. Original credential admission does not own persistent I/O.
        self.auth.guard.take();
        let (transport, driver) = ChannelTransport::bounded_driver();
        let (handle, task) = session::spawn_session(
            TermSize::new(32, 8).map_err(|_| VaultOwnerError::Failed)?,
            64,
            transport,
        );
        let mut state = handle.state.clone();
        let release = self.native_release.take();
        let done = self.actor_done.clone();
        Ok(AuthenticatedProtocolParts::new(handle, async move {
            state
                .wait_for(|state| state.lifecycle != session::Lifecycle::Running)
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            drop(driver);
            task.join()
                .await
                .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            done.notify_one();
            if let Some(release) = release {
                release
                    .await
                    .map_err(|_| ProtocolSessionError::NativeUnknown)?;
            }
            Ok(())
        })
        .with_appearance(self.appearance.clone()))
    }
}
fn session_sink(
    selected: (Uuid, Uuid),
    pending_setup: bool,
    blocked_cleanup: bool,
) -> (SessionSink, Arc<Seen>, tokio::sync::oneshot::Sender<()>) {
    let (auth, seen, cleanup) = sink(selected, false, blocked_cleanup);
    (
        SessionSink {
            auth,
            lock_after_auth: None,
            setup_pending: pending_setup,
            setup_started: Arc::new(tokio::sync::Notify::new()),
            transferred: Arc::new(AtomicUsize::new(0)),
            native_release: None,
            actor_done: Arc::new(tokio::sync::Notify::new()),
            appearance: Default::default(),
        },
        seen,
        cleanup,
    )
}

#[test]
fn actual_ciphertext_session_role_original_epoch_page_cancel_lost_owner_and_native_join()
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
        let encrypted = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let request = op.begin_unlock(
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            encrypted.clone(),
        )?;
        op.confirm_unlock(&store, encrypted, request.ready().await?)
            .await?
            .accept()?;
        let identities = store
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
        let identity = identities.records().next().ok_or("fixture.identity")?.id();
        let hosts = store
            .edit_ssh_hosts(
                store.load_ssh_hosts().await?,
                HostEdit::Create(SshEndpoint::new("fixed", "fixed.example", 22, "user")?),
                Arc::new(|| true),
            )
            .await?;
        let host = hosts.records()[0].id();
        owner
            .begin(p.scope.clone())?
            .prepare_password_identity_write(
                store.load_identity_vault_source(Arc::new(|| true)).await?,
                identity,
            )?
            .write(
                &store,
                SecretBytes::from_owned(b"fixed-target-secret".to_vec())?,
            )
            .await?;
        let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
        let selected = (host, identity);

        // Distinct session gesture authenticates the real ciphertext and returns an owned actor.
        let intent = owner
            .begin_password_session_action_authorized(
                p.scope.clone(),
                host,
                identity,
                Arc::new(|| true),
            )?
            .prepare(&store, source.clone())
            .await?;
        let started = ProtocolSessionReservation::reserve(&p.registry, &p.scope)?.begin()?;
        let (mut adapter, seen, _) = session_sink(selected, false, false);
        adapter.appearance = pulse_store::profile_editor::ProfileAppearance {
            backspace_send: Some("bs".into()),
            delete_send: Some("del".into()),
            ..Default::default()
        };
        let transferred = adapter.transferred.clone();
        let actor_done = adapter.actor_done.clone();
        let (release, pending_native) = tokio::sync::oneshot::channel();
        adapter.native_release = Some(pending_native);
        let owned = intent.start(store.clone(), adapter, started).wait().await?;
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert!(seen.exact.load(Ordering::Acquire));
        assert_eq!(transferred.load(Ordering::SeqCst), 1);
        assert!(owned.is_managed_by(&p.registry));
        assert_eq!(owned.appearance().backspace_send.as_deref(), Some("bs"));
        assert_eq!(owned.appearance().delete_send.as_deref(), Some("del"));
        // Credential-operation admission is free while root transport remains owned.
        drop(owner.begin(p.scope.clone())?);
        let id = owned.id();
        let mut cleanup_state = owned.observe_cleanup();
        drop(owned.cancel());
        await_signal(&actor_done).await?;
        assert_eq!(
            *cleanup_state.borrow(),
            crate::local_session_owner::CleanupState::Closing
        );
        assert!(p.registry.state(p.window, id).is_ok());
        release.send(()).map_err(|_| "fixture.native_release")?;
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            cleanup_state.wait_for(|s| *s == crate::local_session_owner::CleanupState::Reaped),
        )
        .await??;

        // A reservation from another confirmed page cannot receive this identity's authority.
        let intent = owner
            .begin_password_session_action_authorized(
                p.scope.clone(),
                host,
                identity,
                Arc::new(|| true),
            )?
            .prepare(&store, source.clone())
            .await?;
        let window = p.registry.open_window()?;
        let other_page = p.registry.activate_launch_page(window)?;
        let started = ProtocolSessionReservation::reserve(&p.registry, &other_page)?.begin()?;
        let (adapter, seen, _) = session_sink(selected, false, false);
        assert!(
            intent
                .start(store.clone(), adapter, started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.auth.load(Ordering::SeqCst), 0);

        // A completed owner buffered in a lost waiter is closed, not left as an orphan session.
        let intent = owner
            .begin_password_session_action_authorized(
                p.scope.clone(),
                host,
                identity,
                Arc::new(|| true),
            )?
            .prepare(&store, source.clone())
            .await?;
        let started = ProtocolSessionReservation::reserve(&p.registry, &p.scope)?.begin()?;
        let (mut adapter, seen, _) = session_sink(selected, false, false);
        let transferred = adapter.transferred.clone();
        let actor_done = adapter.actor_done.clone();
        let (release, pending_native) = tokio::sync::oneshot::channel();
        adapter.native_release = Some(pending_native);
        let pending_owner = intent.start(store.clone(), adapter, started);
        // close_and_drain returns immediately and sends the owner in the same task poll.
        await_signal(&seen.draining).await?;
        assert_eq!(transferred.load(Ordering::SeqCst), 1);
        drop(pending_owner);
        await_signal(&actor_done).await?;
        release.send(()).map_err(|_| "fixture.lost_owner_native")?;

        // Locking after password authentication cannot publish a terminal from the old epoch.
        let intent = owner
            .begin_password_session_action_authorized(
                p.scope.clone(),
                host,
                identity,
                Arc::new(|| true),
            )?
            .prepare(&store, source.clone())
            .await?;
        let started = ProtocolSessionReservation::reserve(&p.registry, &p.scope)?.begin()?;
        let (mut adapter, seen, _) = session_sink(selected, false, false);
        adapter.lock_after_auth = Some(owner.clone());
        let transferred = adapter.transferred.clone();
        assert!(
            intent
                .start(store.clone(), adapter, started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert_eq!(transferred.load(Ordering::SeqCst), 0);
        let encrypted = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        let op = owner.begin(p.scope.clone())?;
        let request = op.begin_unlock(
            PasswordInput::new(b"fixed-target-master".to_vec())?,
            encrypted.clone(),
        )?;
        op.confirm_unlock(&store, encrypted, request.ready().await?)
            .await?
            .accept()?;

        // A waiter cancelled during terminal setup retains native and credential admission.
        let intent = owner
            .begin_password_session_action_authorized(
                p.scope.clone(),
                host,
                identity,
                Arc::new(|| true),
            )?
            .prepare(&store, source.clone())
            .await?;
        let started = ProtocolSessionReservation::reserve(&p.registry, &p.scope)?.begin()?;
        let (adapter, seen, release_cleanup) = session_sink(selected, true, true);
        let setup_started = adapter.setup_started.clone();
        let transferred = adapter.transferred.clone();
        let task = intent.start(store.clone(), adapter, started);
        await_signal(&setup_started).await?;
        drop(task);
        await_signal(&seen.draining).await?;
        assert!(matches!(
            owner.begin(p.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        assert_eq!(transferred.load(Ordering::SeqCst), 0);
        release_cleanup
            .send(())
            .map_err(|_| "fixture.cleanup_release")?;
        // Shared native drain waits for the entire cleanup including original intent disposal.
        p.registry.seal_native_workers().join().await?;
        drop(owner.begin(p.scope.clone())?);
        owner.close()?;
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
