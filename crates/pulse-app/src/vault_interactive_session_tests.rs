use super::*;
use crate::vault_owner::InteractiveSessionIntent;
use pulse_core::ssh_identity::SshPasswordIdentity;
use pulse_store::ssh_identities::IdentityEdit;
async fn fixture() -> std::result::Result<(Fixture, Uuid), Box<dyn std::error::Error>> {
    let f = Fixture::new().await?;
    let empty = f.store.load_ssh_identities(Arc::new(|| true)).await?;
    let saved = f
        .store
        .edit_ssh_identities(
            empty,
            IdentityEdit::Create(SshPasswordIdentity::new(
                "MFA metadata",
                "user",
                SecretRef::new(f.vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
            )?),
            Arc::new(|| true),
        )
        .await?;
    let id = saved.records().next().ok_or("fixture.metadata")?.id();
    Ok((f, id))
}
async fn prepare(
    f: &Fixture,
    id: Uuid,
) -> std::result::Result<InteractiveSessionIntent, Box<dyn std::error::Error>> {
    Ok(f.owner
        .begin_interactive_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?
        .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
        .prepare(&f.store, f.source().await?)
        .await?)
}
#[test]
fn metadata_without_saved_secret_keeps_original_page_epoch_and_cancellation() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = fixture().await?;
        let other = page()?;
        let gesture = f.owner.begin_interactive_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?;
        assert!(
            gesture
                .begin_session_authorized(&f.owner, &other.scope, Arc::new(|| true))
                .is_err()
        );
        let intent = prepare(&f, id).await?;
        let guard = intent.authorization();
        assert!(guard());
        intent.cancellation().cancel();
        assert!(!guard());
        drop(guard);
        drop(intent);
        let intent = prepare(&f, id).await?;
        let guard = intent.authorization();
        f.owner.lock()?;
        assert!(!guard());
        drop(intent);
        drop(guard);
        other.registry.shutdown()?;
        f.finish().await
    })
}
#[test]
fn old_source_changed_account_and_private_key_roles_cannot_become_interactive() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = fixture().await?;
        assert!(prepare(&f, f.identity).await.is_err());
        let action = f
            .owner
            .begin_interactive_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                id,
                Arc::new(|| true),
            )?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let source = f.source().await?;
        let before = f.store.load_ssh_identities(Arc::new(|| true)).await?;
        f.store
            .edit_ssh_identities(before, IdentityEdit::Remove(id), Arc::new(|| true))
            .await?;
        assert!(action.prepare(&f.store, source).await.is_err());
        f.finish().await
    })
}

#[test]
fn saved_metadata_capture_can_only_be_consumed_as_interactive_before_its_original_deadline()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = fixture().await?;
        let capture = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?;
        let interactive = capture.into_interactive()?;
        assert!(interactive.matches_selection(f.host, id));
        let action =
            interactive.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let source = f.source().await?;
        drop(action.prepare(&f.store, source).await?);
        let expired = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?;
        let expired =
            expired.tighten_deadline(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(expired.into_interactive().is_err());
        let stale = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?;
        f.owner.lock()?;
        assert!(stale.into_interactive().is_err());
        f.finish().await
    })
}

#[test]
fn cold_metadata_click_preserves_deadline_and_cancellation_before_interactive_conversion()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = fixture().await?;
        let cold = f.owner.begin_cold_saved_session_intent_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(|| true),
        )?;
        let gesture = cold
            .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .into_interactive()?;
        assert!(gesture.matches_selection(f.host, id));
        let action =
            gesture.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let source = f.source().await?;
        drop(action.prepare(&f.store, source).await?);
        let expired = f
            .owner
            .begin_cold_saved_session_intent_authorized(
                f.page.scope.clone(),
                f.host,
                id,
                Arc::new(|| true),
            )?
            .tighten_deadline(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(
            expired
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let revoked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let stop = revoked.clone();
        let cancelled = f.owner.begin_cold_saved_session_intent_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(move || !stop.load(std::sync::atomic::Ordering::SeqCst)),
        )?;
        revoked.store(true, std::sync::atomic::Ordering::SeqCst);
        assert!(
            cancelled
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        f.finish().await
    })
}

#[test]
fn temporary_interactive_source_revocation_before_and_after_prepare_is_enforced() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = fixture().await?;
        let before = f.store.load_ssh_hosts().await?;
        for revoke_before in [true, false] {
            let target = f.store.temporary_ssh_target(
                SshEndpoint::new("temporary MFA", "temporary.example", 22, "user")?,
                Arc::new(|| true),
            );
            let source = f
                .store
                .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
                .await?;
            assert!(source.is_temporary());
            let action = f
                .owner
                .begin_interactive_session_gesture_authorized(
                    f.page.scope.clone(),
                    target.id(),
                    id,
                    Arc::new(|| true),
                )?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
            if revoke_before {
                target.revoke();
                assert!(action.prepare(&f.store, source).await.is_err());
            } else {
                let intent = action.prepare(&f.store, source.clone()).await?;
                let guard = intent.authorization();
                assert!(guard());
                target.revoke();
                assert!(!guard());
                assert!(
                    f.store
                        .validate_ssh_key_target_source(source, Arc::new(|| true))
                        .await
                        .is_err()
                );
                drop(intent);
            }
        }
        let after = f.store.load_ssh_hosts().await?;
        assert_eq!(
            before
                .records()
                .iter()
                .map(|host| host.id())
                .collect::<Vec<_>>(),
            after
                .records()
                .iter()
                .map(|host| host.id())
                .collect::<Vec<_>>()
        );
        f.finish().await
    })
}
