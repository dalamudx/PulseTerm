//! Independent joint capture must preserve original page/epoch and source authority.
use super::*;
use crate::vault_owner::{AgentInteractiveSessionGesture, AgentInteractiveSessionSink};
use std::time::{Duration, Instant};
impl AgentInteractiveSessionSink for RejectUnexpectedSign {}
fn capture(
    f: &Fixture,
    id: Uuid,
) -> std::result::Result<AgentInteractiveSessionGesture, VaultOwnerError> {
    f.owner.begin_agent_interactive_session_gesture_authorized(
        f.page.scope.clone(),
        f.host,
        id,
        Arc::new(|| true),
    )
}
#[test]
fn agent_joint_original_page_epoch_cancel_and_identity_kind_remain_bound() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = agent_fixture().await?;
        let foreign = page()?;
        assert!(capture(&f, id)?.matches_selection(f.host, id));
        assert!(
            capture(&f, id)?
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            capture(&f, id)?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let old = capture(&f, id)?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let action = capture(&f, id)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        restore_same_dek(&f).await?;
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        let wrong_kind = capture(&f, f.identity)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        assert!(
            wrong_kind
                .prepare(&f.store, f.source().await?)
                .await
                .is_err()
        );
        let intent = capture(&f, id)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        assert!(intent.authorization()());
        assert!(intent.deadline() > Instant::now() + Duration::from_secs(250));
        let guard = intent.authorization();
        intent.cancellation().cancel();
        assert!(!guard());
        drop(guard);
        drop(intent);
        foreign.registry.shutdown()?;
        f.finish().await
    })
}
#[test]
fn agent_joint_changed_source_and_foreign_reservation_make_zero_sign_calls() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = agent_fixture().await?;
        let intent = capture(&f, id)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        let rows = f.store.load_ssh_agent_identities(Arc::new(|| true)).await?;
        f.store
            .edit_ssh_agent_identity(
                rows,
                AgentIdentityEdit::Update(
                    id,
                    pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                        "Changed joint Agent",
                        "user",
                        "ssh-ed25519",
                        &format!("SHA256:{}", "A".repeat(43)),
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        let calls = Arc::new(AtomicUsize::new(0));
        let mut sink = RejectUnexpectedSign {
            calls: calls.clone(),
        };
        let mut started =
            Some(ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?);
        assert!(
            intent
                .authenticate_terminal(&f.store, &mut sink, &mut started)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        started
            .take()
            .ok_or("fixture.start")?
            .finish_unregistered(async { Ok(()) })
            .join()
            .await?;
        drop(intent);
        let current = capture(&f, id)?
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, f.source().await?)
            .await?;
        let foreign = page()?;
        let mut started =
            Some(ProtocolSessionReservation::reserve(&foreign.registry, &foreign.scope)?.begin()?);
        assert!(
            current
                .authenticate_terminal(&f.store, &mut sink, &mut started)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        started
            .take()
            .ok_or("fixture.start")?
            .finish_unregistered(async { Ok(()) })
            .join()
            .await?;
        drop(current);
        foreign.registry.shutdown()?;
        f.finish().await
    })
}
