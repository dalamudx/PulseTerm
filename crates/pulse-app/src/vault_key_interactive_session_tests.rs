//! Joint authentication cannot reuse a normal private-key intent or a later epoch.
use super::*;
use crate::vault_owner::PrivateKeyInteractiveSessionGesture;
fn joint(f: &Fixture) -> std::result::Result<PrivateKeyInteractiveSessionGesture, VaultOwnerError> {
    f.owner
        .begin_private_key_interactive_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )
}
#[test]
fn joint_private_key_capture_checks_original_owner_page_selection_epoch_and_deadline() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let foreign = page()?;
        let other = PageVaultOwner::new(f.page.registry.clone());
        assert!(joint(&f)?.matches_selection(f.host, f.identity));
        assert!(!joint(&f)?.matches_selection(f.host, Uuid::now_v7()));
        assert!(
            joint(&f)?
                .begin_session_authorized(&other, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            joint(&f)?
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            joint(&f)?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let mut expired = joint(&f)?;
        expired.fixture_expire();
        assert!(
            expired
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let old = joint(&f)?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let action =
            joint(&f)?.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let wrong = f.store.temporary_ssh_target(
            SshEndpoint::new("other", "other.example", 22, "user")?,
            Arc::new(|| true),
        );
        let source = f
            .store
            .load_temporary_ssh_key_target_source(wrong, Arc::new(|| true))
            .await?;
        assert!(action.prepare(&f.store, source).await.is_err());
        f.finish().await
    })
}
#[test]
fn joint_private_key_temporary_revocation_before_and_after_prepare_denies_native_use() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        for before in [true, false] {
            let target = f.store.temporary_ssh_target(
                SshEndpoint::new("joint", "temporary.example", 22, "user")?,
                Arc::new(|| true),
            );
            let source = f
                .store
                .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
                .await?;
            let action = f
                .owner
                .begin_private_key_interactive_session_gesture_authorized(
                    f.page.scope.clone(),
                    target.id(),
                    f.identity,
                    Arc::new(|| true),
                )?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
            if before {
                target.revoke();
                assert!(action.prepare(&f.store, source).await.is_err());
            } else {
                let intent = action.prepare(&f.store, source).await?;
                assert!((intent.authorization())());
                assert!(
                    intent
                        .deadline()
                        .saturating_duration_since(std::time::Instant::now())
                        .as_secs()
                        > 290
                );
                target.revoke();
                assert!(!(intent.authorization())());
                drop(intent);
            }
        }
        let normal = capture(&f)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        assert!(
            normal
                .deadline()
                .saturating_duration_since(std::time::Instant::now())
                .as_secs()
                <= 5
        );
        drop(normal);
        let action =
            joint(&f)?.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let intent = action.prepare(&f.store, f.source().await?).await?;
        let guard = intent.authorization();
        assert!(guard());
        restore_same_dek(&f).await?;
        assert!(!guard());
        drop(intent);
        f.finish().await
    })
}

#[test]
fn cli_joint_private_key_capture_keeps_original_epoch_deadline_and_cancellation() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let capture = || {
            f.owner.begin_saved_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                f.identity,
                Arc::new(|| true),
            )
        };
        let expired = capture()?
            .tighten_deadline(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(expired.into_key_interactive().is_err());
        let allowed = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let current = allowed.clone();
        let revoked = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || current.load(std::sync::atomic::Ordering::SeqCst)),
        )?;
        allowed.store(false, std::sync::atomic::Ordering::SeqCst);
        assert!(revoked.into_key_interactive().is_err());
        let old = capture()?.into_key_interactive()?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let intent = capture()?
            .into_key_interactive()?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        assert!(intent.authorization()());
        drop(intent);
        f.finish().await
    })
}
#[test]
fn cli_cold_joint_private_key_metadata_requires_separate_confirmation_and_keeps_its_deadline()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let metadata = || {
            f.owner.begin_cold_saved_session_intent_authorized(
                f.page.scope.clone(),
                f.host,
                f.identity,
                Arc::new(|| true),
            )
        };
        let expired = metadata()?
            .tighten_deadline(std::time::Instant::now() - std::time::Duration::from_secs(1));
        assert!(
            expired
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let cold = metadata()?;
        restore_same_dek(&f).await?;
        let selected = cold
            .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .into_key_interactive()?;
        assert!(selected.matches_selection(f.host, f.identity));
        restore_same_dek(&f).await?;
        assert!(
            selected
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        f.finish().await
    })
}
