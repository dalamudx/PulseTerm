//! CLI-neutral saved Agent selection inherits the original page, expiry and Vault epoch.
use super::*;
use pulse_store::ssh_agent_identities::AgentIdentityEdit;
#[path = "vault_saved_alias_gesture_tests.rs"]
mod alias;

async fn agent_identity(f: &Fixture) -> std::result::Result<Uuid, Box<dyn std::error::Error>> {
    let snapshot = f
        .store
        .edit_ssh_agent_identity(
            f.store.load_ssh_agent_identities(Arc::new(|| true)).await?,
            AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                "CLI selected Agent",
                "user",
                "ssh-ed25519",
                &format!("SHA256:{}", "A".repeat(43)),
            )?),
            Arc::new(|| true),
        )
        .await?;
    Ok(snapshot.records().next().ok_or("fixture.agent")?.id())
}

#[test]
fn saved_agent_original_epoch_cancel_expiry_and_no_metadata_writes() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let id = agent_identity(&f).await?;
        let revision = f
            .store
            .load_ssh_agent_identities(Arc::new(|| true))
            .await?
            .revision();
        let source = f.source().await?;
        let chosen = saved(&f, id)?.select_source(&source)?;
        let SavedSessionSelection::Agent(gesture) = chosen else {
            return Err("fixture.kind".into());
        };
        assert!(gesture.matches_selection(f.host, id));
        assert!(!gesture.matches_selection(f.host, f.identity));
        restore_same_dek(&f).await?;
        assert!(
            gesture
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let mut expired = saved(&f, id)?;
        expired.fixture_expire();
        assert!(expired.select_source(&source).is_err());
        let live = Arc::new(AtomicBool::new(true));
        let check = live.clone();
        let cancelled = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(move || check.load(Ordering::Acquire)),
        )?;
        live.store(false, Ordering::Release);
        assert!(cancelled.select_source(&source).is_err());
        let SavedSessionSelection::Agent(fresh) = saved(&f, id)?.select_source(&source)? else {
            return Err("fixture.kind".into());
        };
        let foreign = page()?;
        assert!(
            fresh
                .begin_session_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        let SavedSessionSelection::Agent(fresh) = saved(&f, id)?.select_source(&source)? else {
            return Err("fixture.kind".into());
        };
        let action = fresh.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let intent = action.prepare(&f.store, source).await?;
        drop(intent);
        assert_eq!(
            f.store
                .load_ssh_agent_identities(Arc::new(|| true))
                .await?
                .revision(),
            revision
        );
        f.finish().await
    })
}

#[test]
fn saved_agent_collision_tombstone_and_metadata_replacement_reject() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let id = agent_identity(&f).await?;
        // Only this task's real database is changed by the independent fixture process.
        let status = std::process::Command::new("python3").env("PYTHONDONTWRITEBYTECODE", "1")
            .args(["-c", "import sqlite3,sys\nwith sqlite3.connect(sys.argv[1]) as c: c.execute('UPDATE agent_identities SET id=? WHERE id=?',(sys.argv[2],sys.argv[3]))"])
            .arg(f._root.path().join("state/state.db")).arg(f.identity.to_string()).arg(id.to_string()).status()?;
        assert!(status.success(), "fixture.external_agent_collision");
        let source = f.source().await?;
        assert!(saved(&f, f.identity)?.select_source(&source).is_err());
        f.store.edit_ssh_agent_identity(
            f.store.load_ssh_agent_identities(Arc::new(|| true)).await?, AgentIdentityEdit::Remove(f.identity), Arc::new(|| true),
        ).await?;
        assert!(saved(&f, id)?.select_source(&f.source().await?).is_err());
        let id = agent_identity(&f).await?;
        let source = f.source().await?;
        let SavedSessionSelection::Agent(gesture) = saved(&f, id)?.select_source(&source)? else { return Err("fixture.kind".into()); };
        let action = gesture.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        f.store.edit_ssh_agent_identity(
            f.store.load_ssh_agent_identities(Arc::new(|| true)).await?,
            AgentIdentityEdit::Update(id, pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                "Replaced Agent", "user", "ssh-ed25519", &format!("SHA256:{}A", "B".repeat(42)),
            )?), Arc::new(|| true),
        ).await?;
        assert!(action.prepare(&f.store, source).await.is_err());
        f.finish().await
    })
}

#[test]
fn route_handoff_only_tightens_original_page_epoch_deadline_and_cancellation() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        assert!(
            saved(&f, f.identity)?
                .tighten_deadline(std::time::Instant::now())
                .select_source(&f.source().await?)
                .is_err()
        );
        let foreign = page()?;
        assert!(
            saved(&f, f.identity)?
                .restrict_route_authorization(&foreign.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            saved(&f, f.identity)?
                .restrict_route_authorization(&f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let original = Arc::new(AtomicBool::new(true));
        let additional = Arc::new(AtomicBool::new(true));
        let a = original.clone();
        let b = additional.clone();
        let capture = f.owner.begin_routed_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || a.load(Ordering::Acquire)),
        )?;
        let restricted = capture.restrict_route_authorization(
            &f.page.scope,
            Arc::new(move || b.load(Ordering::Acquire)),
        )?;
        assert!(restricted.permits_route_read(&f.page.scope));
        additional.store(false, Ordering::Release);
        assert!(!restricted.permits_route_read(&f.page.scope));
        additional.store(true, Ordering::Release);
        original.store(false, Ordering::Release);
        assert!(!restricted.permits_route_read(&f.page.scope));
        let capture = saved(&f, f.identity)?
            .restrict_route_authorization(&f.page.scope, Arc::new(|| true))?;
        restore_same_dek(&f).await?;
        assert!(!capture.permits_route_read(&f.page.scope));
        let mut expired = saved(&f, f.identity)?;
        expired.fixture_expire();
        assert!(
            expired
                .restrict_route_authorization(&f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let mut restricted = saved(&f, f.identity)?
            .restrict_route_authorization(&f.page.scope, Arc::new(|| true))?;
        restricted.fixture_expire();
        assert!(!restricted.permits_route_read(&f.page.scope));
        drop(foreign);
        f.finish().await
    })
}

#[test]
fn cold_saved_intent_unlock_requires_distinct_confirmation_and_freezes_clicked_epoch() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let id = agent_identity(&f).await?;
        let source = f.source().await?;
        let revision = f
            .store
            .load_ssh_agent_identities(Arc::new(|| true))
            .await?
            .revision();
        f.owner.lock()?;
        let intent = || {
            f.owner.begin_cold_saved_session_intent_authorized(
                f.page.scope.clone(),
                f.host,
                id,
                Arc::new(|| true),
            )
        };
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let pending = intent()?;
        // Waiting metadata has no operation credit and can coexist with Vault unlock.
        drop(f.owner.begin(f.page.scope.clone())?);
        restore_same_dek(&f).await?;
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let clicked = pending.confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        let SavedSessionSelection::Agent(gesture) = clicked.select_source(&source)? else {
            return Err("fixture.kind".into());
        };
        assert!(gesture.matches_selection(f.host, id));
        assert!(!gesture.matches_selection(f.host, f.identity));
        restore_same_dek(&f).await?;
        assert!(
            gesture
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert_eq!(
            revision,
            f.store
                .load_ssh_agent_identities(Arc::new(|| true))
                .await?
                .revision()
        );
        f.finish().await
    })
}

#[test]
fn cold_saved_intent_foreign_owner_page_expiry_and_both_cancellations_reject() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let id = agent_identity(&f).await?;
        let source = f.source().await?;
        let intent = || {
            f.owner.begin_cold_saved_session_intent_authorized(
                f.page.scope.clone(),
                f.host,
                id,
                Arc::new(|| true),
            )
        };
        let foreign = page()?;
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &foreign.scope, Arc::new(|| true))
                .is_err()
        );
        let other = PageVaultOwner::new(f.owner.inner.registry.clone());
        assert!(
            intent()?
                .confirm_authorized(&other, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            intent()?
                .tighten_deadline(std::time::Instant::now())
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        assert!(
            f.owner
                .begin_cold_saved_session_intent_authorized(
                    f.page.scope.clone(),
                    Uuid::nil(),
                    id,
                    Arc::new(|| true),
                )
                .is_err()
        );
        let alive = Arc::new(AtomicBool::new(true));
        let original = alive.clone();
        let pending = f.owner.begin_cold_saved_session_intent_authorized(
            f.page.scope.clone(),
            f.host,
            id,
            Arc::new(move || original.load(Ordering::Acquire)),
        )?;
        alive.store(false, Ordering::Release);
        assert!(
            pending
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let clicked = Arc::new(AtomicBool::new(true));
        let guard = clicked.clone();
        let captured = intent()?.confirm_authorized(
            &f.owner,
            &f.page.scope,
            Arc::new(move || guard.load(Ordering::Acquire)),
        )?;
        clicked.store(false, Ordering::Release);
        assert!(captured.select_source(&source).is_err());
        let original = alive.clone();
        alive.store(true, Ordering::Release);
        let captured = f
            .owner
            .begin_cold_saved_session_intent_authorized(
                f.page.scope.clone(),
                f.host,
                id,
                Arc::new(move || original.load(Ordering::Acquire)),
            )?
            .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        alive.store(false, Ordering::Release);
        assert!(captured.select_source(&source).is_err());
        drop(other);
        drop(foreign);
        f.finish().await
    })
}
