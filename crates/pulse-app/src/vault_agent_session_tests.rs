//! Original page/epoch and source guards only. Real signing/peer tests live in pulse-ssh.
use super::*;
#[path = "vault_agent_interactive_session_tests.rs"]
mod interactive;
#[path = "vault_agent_route_tests.rs"]
mod route;
use crate::vault_owner::{AgentSessionGesture, AgentSessionIntent, AgentSessionSink};
use pulse_store::ssh_agent_identities::AgentIdentityEdit;
async fn agent_fixture() -> std::result::Result<(Fixture, Uuid), Box<dyn std::error::Error>> {
    let f = Fixture::new().await?;
    let current = f.store.load_ssh_agent_identities(Arc::new(|| true)).await?;
    let saved = f
        .store
        .edit_ssh_agent_identity(
            current,
            AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                "Selected Agent",
                "user",
                "ssh-ed25519",
                &format!("SHA256:{}", "A".repeat(43)),
            )?),
            Arc::new(|| true),
        )
        .await?;
    let id = saved.records().next().ok_or("fixture.agent")?.id();
    Ok((f, id))
}
fn agent_capture(
    f: &Fixture,
    id: Uuid,
) -> std::result::Result<AgentSessionGesture, VaultOwnerError> {
    f.owner.begin_agent_session_gesture_authorized(
        f.page.scope.clone(),
        f.host,
        id,
        Arc::new(|| true),
    )
}
async fn agent_prepare(
    f: &Fixture,
    id: Uuid,
) -> std::result::Result<AgentSessionIntent, Box<dyn std::error::Error>> {
    Ok(agent_capture(f, id)?
        .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
        .prepare(&f.store, f.source().await?)
        .await?)
}
#[test]
fn agent_original_page_owner_and_identity_kind_reject_without_secret_authority() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = agent_fixture().await?;
        let foreign = page()?;
        let other = PageVaultOwner::new(f.page.registry.clone());
        assert!(agent_capture(&f, id)?.matches_selection(f.host, id));
        assert!(
            agent_capture(&f, id)?
                .begin_session_authorized(&other, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            agent_capture(&f, id)?
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            agent_capture(&f, id)?
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let action = agent_capture(&f, f.identity)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        let intent = agent_prepare(&f, id).await?;
        let guard = intent.authorization();
        assert!(guard());
        intent.cancellation().cancel();
        assert!(!guard());
        drop(intent);
        drop(guard);
        other.close()?;
        foreign.registry.shutdown()?;
        f.finish().await
    })
}
#[test]
fn agent_original_epoch_reunlock_and_cancel_revoke_before_signing() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = agent_fixture().await?;
        let old = agent_capture(&f, id)?;
        restore_same_dek(&f).await?;
        assert!(
            old.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let action = agent_capture(&f, id)?.begin_session_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(|| true),
        )?;
        restore_same_dek(&f).await?;
        assert!(action.prepare(&f.store, f.source().await?).await.is_err());
        let intent = agent_prepare(&f, id).await?;
        let guard = intent.authorization();
        assert!(guard());
        f.owner.lock()?;
        assert!(!guard());
        drop(intent);
        drop(guard);
        restore_same_dek(&f).await?;
        let renewed = agent_prepare(&f, id).await?;
        assert!(renewed.authorization()());
        drop(renewed);
        f.finish().await
    })
}
struct RejectUnexpectedSign {
    calls: Arc<AtomicUsize>,
}
impl AgentSessionSink for RejectUnexpectedSign {
    fn matches_target(&self, _source: &SshKeyTargetSource, _host: Uuid, _identity: Uuid) -> bool {
        true
    }
    fn retain_native_authorization(
        &mut self,
        _guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::result::Result<(), VaultOwnerError> {
        Ok(())
    }
    fn authenticate_agent<'a>(
        &'a mut self,
        _identity: &'a pulse_core::ssh_agent_identity::SshAgentIdentity,
        _guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::pin::Pin<
        Box<
            dyn std::future::Future<Output = std::result::Result<bool, VaultOwnerError>>
                + Send
                + 'a,
        >,
    > {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn prepare_authenticated_terminal(
        &mut self,
        _guard: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>,
    > {
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn take_authenticated_root_parts(
        &mut self,
    ) -> std::result::Result<crate::vault_owner::AuthenticatedProtocolParts, VaultOwnerError> {
        Err(VaultOwnerError::Denied)
    }
}
#[test]
fn agent_changed_metadata_and_foreign_native_reservation_reject_before_any_sign() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, id) = agent_fixture().await?;
        let intent = agent_prepare(&f, id).await?;
        let rows = f.store.load_ssh_agent_identities(Arc::new(|| true)).await?;
        f.store
            .edit_ssh_agent_identity(
                rows,
                AgentIdentityEdit::Update(
                    id,
                    pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                        "Changed Agent",
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
        let current = agent_prepare(&f, id).await?;
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
