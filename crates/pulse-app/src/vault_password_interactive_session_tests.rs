//! Original approval and source boundaries for explicit saved-password joint authentication.
use super::*;
use crate::vault_owner::{PasswordInteractiveSessionGesture, PasswordSessionGesture};
use pulse_core::ssh_identity::SshPasswordIdentity;
use pulse_store::ssh_identities::IdentityEdit;
fn joint(f: &Fixture, identity: Uuid) -> Result<PasswordInteractiveSessionGesture> {
    f.owner
        .begin_password_interactive_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            identity,
            Arc::new(|| true),
        )
}
async fn password_identity(f: &Fixture) -> std::result::Result<Uuid, Box<dyn std::error::Error>> {
    let secret = Uuid::now_v7();
    f.seal(secret, SecretKind::SshPassword).await?;
    let identities = f
        .store
        .edit_ssh_identities(
            f.store.load_ssh_identities(Arc::new(|| true)).await?,
            IdentityEdit::Create(SshPasswordIdentity::new(
                "joint password",
                "user",
                SecretRef::new(f.vault, secret, SecretKind::SshPassword, false)?,
            )?),
            Arc::new(|| true),
        )
        .await?;
    Ok(identities.records().last().ok_or("fixture.identity")?.id())
}
#[test]
fn password_joint_original_page_owner_epoch_and_approval_cannot_be_replaced() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let identity = password_identity(&f).await?;
        let foreign = page()?;
        let other = PageVaultOwner::new(f.page.registry.clone());
        assert!(joint(&f, identity)?.matches_selection(f.host, identity));
        assert!(!joint(&f, identity)?.matches_selection(f.host, Uuid::now_v7()));
        drop(f.owner.begin(f.page.scope.clone())?);
        assert!(
            joint(&f, identity)?
                .begin_session_authorized(&other, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            joint(&f, identity)?
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            joint(&f, identity)?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let mut expired = joint(&f, identity)?;
        expired.fixture_expire();
        assert!(
            expired
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let old = joint(&f, identity)?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        f.finish().await
    })
}
#[test]
fn password_joint_fixed_click_deadline_does_not_extend_ordinary_password_use() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let identity = password_identity(&f).await?;
        let ordinary: PasswordSessionGesture = f.owner.begin_password_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            identity,
            Arc::new(|| true),
        )?;
        let ordinary = ordinary
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(
                &f.store,
                f.store.load_ssh_target_source(Arc::new(|| true)).await?,
            )
            .await?;
        assert!(
            ordinary
                .deadline()
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
                <= 5
        );
        drop(ordinary);
        let gesture = joint(&f, identity)?;
        let clicked = std::time::Instant::now();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let intent = gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(
                &f.store,
                f.store.load_ssh_target_source(Arc::new(|| true)).await?,
            )
            .await?;
        assert!(intent.deadline() <= clicked + std::time::Duration::from_secs(300));
        assert!(
            intent
                .deadline()
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
                > 290
        );
        let allowed = intent.authorization();
        assert!(allowed());
        intent.cancellation().cancel();
        assert!(!allowed());
        drop(intent);
        f.finish().await
    })
}
#[test]
fn password_joint_rejects_temporary_stale_sources_and_later_unlocks() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let identity = password_identity(&f).await?;
        let target = f.store.temporary_ssh_target(
            SshEndpoint::new("temporary", "temporary.example", 22, "user")?,
            Arc::new(|| true),
        );
        let temporary = f
            .store
            .load_temporary_ssh_key_target_source(target, Arc::new(|| true))
            .await?;
        let action = joint(&f, identity)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        assert!(
            action
                .prepare(&f.store, temporary.password_source().clone())
                .await
                .is_err()
        );
        let old = f.store.load_ssh_target_source(Arc::new(|| true)).await?;
        password_identity(&f).await?;
        let action = joint(&f, identity)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        assert!(action.prepare(&f.store, old).await.is_err());
        let intent = joint(&f, identity)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(
                &f.store,
                f.store.load_ssh_target_source(Arc::new(|| true)).await?,
            )
            .await?;
        let allowed = intent.authorization();
        restore_same_dek(&f).await?;
        assert!(!allowed());
        drop(intent);
        f.finish().await
    })
}

#[test]
fn temporary_password_joint_keeps_source_epoch_deadline_and_zero_use_after_revocation() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let identity = password_identity(&f).await?;
        let target = f.store.temporary_ssh_target(
            SshEndpoint::new("Temporary password MFA", "localhost", 2222, "user")?,
            Arc::new(|| true),
        );
        let source = f
            .store
            .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
            .await?
            .password_source()
            .clone();
        let action = || {
            let gesture = f
                .owner
                .begin_password_interactive_session_gesture_authorized(
                    f.page.scope.clone(),
                    target.id(),
                    identity,
                    Arc::new(|| true),
                )
                .map_err(|error| format!("temporary.capture: {error}"))?;
            gesture
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .map_err(|error| format!("temporary.approve: {error}"))
        };
        assert!(action()?.prepare(&f.store, source.clone()).await.is_err());
        assert!(
            action()?
                .prepare_temporary(
                    &f.store,
                    f.store.load_ssh_target_source(Arc::new(|| true)).await?
                )
                .await
                .is_err()
        );
        let wrong = joint(&f, identity)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        assert!(
            wrong
                .prepare_temporary(&f.store, source.clone())
                .await
                .is_err()
        );
        let old = action()?;
        restore_same_dek(&f).await?;
        assert!(
            old.prepare_temporary(&f.store, source.clone())
                .await
                .is_err()
        );
        let captured = action()?;
        let clicked = std::time::Instant::now();
        tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        let cancelled = captured
            .prepare_temporary(&f.store, source.clone())
            .await
            .map_err(|error| format!("temporary.cancel_prepare: {error}"))?;
        assert!(cancelled.deadline() <= clicked + std::time::Duration::from_secs(300));
        let allowed = cancelled.authorization();
        assert!(allowed());
        cancelled.cancellation().cancel();
        assert!(!allowed());
        drop(cancelled);
        drop(allowed);
        let intent = action()?
            .prepare_temporary(&f.store, source.clone())
            .await
            .map_err(|error| format!("temporary.live_prepare: {error}"))?;
        assert_eq!(intent.selected_source().1, (target.id(), identity));
        assert!(intent.authorization()());
        target.revoke();
        assert!(!intent.authorization()());
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let started = crate::protocol_session_owner::ProtocolSessionReservation::reserve(
            &f.page.registry,
            &f.page.scope,
        )
        .map_err(|error| format!("temporary.reserve: {error}"))?
        .begin()
        .map_err(|error| format!("temporary.start: {error}"))?;
        assert!(
            intent
                .start(f.store.clone(), NoPasswordUse(calls.clone()), started)
                .wait()
                .await
                .is_err()
        );
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert!(action()?.prepare_temporary(&f.store, source).await.is_err());
        f.finish()
            .await
            .map_err(|error| format!("temporary.finish: {error}").into())
    })
}
struct NoPasswordUse(Arc<std::sync::atomic::AtomicUsize>);
impl crate::vault_owner::PasswordTargetSink for NoPasswordUse {
    fn matches_target(
        &self,
        _: &pulse_store::ssh_target_source::SshTargetSource,
        _: Uuid,
        _: Uuid,
    ) -> bool {
        true
    }
    fn retain_native_authorization(&mut self, _: crate::vault_owner::Authorize) -> Result<()> {
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        _: &'a str,
        _: &'a str,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<bool>> + Send + 'a>> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn close_and_drain(
        &mut self,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Ok(()) })
    }
}
impl crate::vault_owner::PasswordSessionSink for NoPasswordUse {
    fn prepare_authenticated_terminal(
        &mut self,
        _: crate::vault_owner::Authorize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn take_authenticated_root_parts(
        &mut self,
    ) -> Result<crate::vault_owner::AuthenticatedProtocolParts> {
        Err(VaultOwnerError::Denied)
    }
}
impl crate::vault_owner::PasswordInteractiveSessionSink for NoPasswordUse {}
