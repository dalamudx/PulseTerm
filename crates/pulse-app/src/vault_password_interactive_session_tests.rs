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
