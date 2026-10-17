//! Real Vault epochs and source/root boundaries; network authentication is checked separately by the real-peer probe.
#[path = "vault_agent_session_tests.rs"]
mod agent;
#[path = "vault_interactive_session_tests.rs"]
mod interactive;
#[path = "vault_key_interactive_session_tests.rs"]
mod joint;
#[test]
fn temporary_private_key_intent_revocation_reaches_native_guard_after_prepare() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let target = f.store.temporary_ssh_target(
            SshEndpoint::new("temporary", "temporary.example", 22, "user")?,
            Arc::new(|| true),
        );
        let source = f
            .store
            .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
            .await?;
        let action = f
            .owner
            .begin_private_key_session_gesture_authorized(
                f.page.scope.clone(),
                target.id(),
                f.identity,
                Arc::new(|| true),
            )?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let intent = action.prepare(&f.store, source).await?;
        let guard = intent.authorization();
        assert!(guard());
        target.revoke();
        assert!(!guard());
        drop(intent);
        f.finish().await
    })
}

use super::*;
use crate::vault_owner::PrivateKeySessionGesture;
#[cfg(unix)]
#[path = "vault_saved_session_gesture_tests.rs"]
mod saved_selection;
fn capture(f: &Fixture) -> std::result::Result<PrivateKeySessionGesture, VaultOwnerError> {
    f.owner.begin_private_key_session_gesture_authorized(
        f.page.scope.clone(),
        f.host,
        f.identity,
        Arc::new(|| true),
    )
}
async fn restore_same_dek(f: &Fixture) -> TestResult {
    let scope = f
        .owner
        .inner
        .guard()?
        .scope
        .clone()
        .ok_or("fixture.scope")?;
    scope.lock()?;
    let source = f.source().await?;
    let key = pulse_store::vault_wrap::recover_validation_dek(
        b"fixed-private-preparation-master",
        f.vault,
        source.vault().envelope().ok_or("fixture.envelope")?,
    )?;
    let mut permit = scope.begin()?;
    let mut candidate = permit.worker()?.finish(key)?;
    scope.confirm(&mut permit, &mut candidate)?;
    Ok(())
}
#[test]
fn private_key_gesture_original_owner_page_selection_and_deadline_reject_without_io() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let foreign = page()?;
        let other = PageVaultOwner::new(f.page.registry.clone());
        let gesture = capture(&f)?;
        assert!(gesture.matches_selection(f.host, f.identity));
        assert!(!gesture.matches_selection(f.host, Uuid::now_v7()));
        // Holding or dropping the key-free selection leaves the native operation slot available.
        drop(f.owner.begin(f.page.scope.clone())?);
        drop(gesture);
        assert!(
            capture(&f)?
                .begin_session_authorized(&other, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            capture(&f)?
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            capture(&f)?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let mut expired = capture(&f)?;
        expired.fixture_expire();
        assert!(
            expired
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let original = Arc::new(AtomicBool::new(true));
        let flag = original.clone();
        let gesture = f.owner.begin_private_key_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || flag.load(Ordering::Acquire)),
        )?;
        original.store(false, Ordering::Release);
        assert!(
            gesture
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let old = capture(&f)?;
        f.page.documents.started(f.page.window)?;
        f.owner.retire_page()?;
        let nonce = f.page.documents.finished(f.page.window)?;
        f.page.documents.confirm(f.page.window, nonce)?;
        let current = f.page.documents.current(f.page.window, nonce)?;
        assert!(
            old.begin_session_authorized(&f.owner, &current, Arc::new(|| true))
                .is_err()
        );
        assert!(
            f.owner
                .begin_private_key_session_gesture_authorized(
                    current.clone(),
                    f.host,
                    f.identity,
                    Arc::new(|| true)
                )
                .is_ok()
        );
        let ended = f.owner.begin_private_key_session_gesture_authorized(
            current.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )?;
        f.owner.close()?;
        assert!(
            ended
                .begin_session_authorized(&f.owner, &current, Arc::new(|| true))
                .is_err()
        );
        other.close()?;
        foreign.registry.shutdown()?;
        f.finish().await
    })
}
#[test]
fn private_key_gesture_same_dek_reunlock_cannot_upgrade_before_or_after_consumption() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let old = capture(&f)?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let action =
            capture(&f)?.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        // The original action keeps its operation; restore the real controller without borrowing its slot.
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        restore_same_dek(&f).await?;
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        // Rejected before source I/O: no native work remains to hold the original operation.
        drop(f.owner.begin(f.page.scope.clone())?);
        let started =
            ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?;
        let intent = capture(&f)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        let sink = Sink::new(&f);
        let seen = sink.seen.clone();
        let owned = intent.start(f.store.clone(), sink, started).wait().await?;
        let mut root = crate::local_session_root::SessionRoot::new(
            f.page.registry.clone(),
            tokio::runtime::Handle::current(),
        );
        let id = root.adopt_protocol(owned)?;
        root.close_protocol(id)?.join().await?;
        assert_eq!(root.collect_reaped(), 1);
        assert_eq!(seen.auth.load(Ordering::SeqCst), 1);
        assert_eq!(seen.transferred.load(Ordering::SeqCst), 1);
        f.finish().await
    })
}
#[test]
fn private_key_gesture_approval_revocation_and_changed_source_reject_before_auth() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let approved = Arc::new(AtomicBool::new(true));
        let flag = approved.clone();
        let action = capture(&f)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(move || flag.load(Ordering::Acquire)),
        )?;
        approved.store(false, Ordering::Release);
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        let source = f.source().await?;
        let action =
            capture(&f)?.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        f.store
            .edit_ssh_hosts(
                source.hosts().clone(),
                HostEdit::Create(SshEndpoint::new(
                    "changed",
                    "different.example",
                    22,
                    "user",
                )?),
                Arc::new(|| true),
            )
            .await?;
        assert!(action.prepare(&f.store, source).await.is_err());
        f.owner.lock()?;
        assert!(capture(&f).is_err());
        f.finish().await
    })
}
