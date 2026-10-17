use super::*;
use pulse_store::ssh_profiles::ProfileEdit;
async fn default_agent(f: &Fixture) -> std::result::Result<Uuid, Box<dyn std::error::Error>> {
    let id = agent_identity(f).await?;
    let snapshot = f
        .store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let profile = snapshot
        .profiles()
        .find(|p| p.endpoint_id() == f.host)
        .ok_or("fixture.profile")?
        .metadata()
        .id();
    f.store
        .edit_ssh_profile(
            snapshot,
            ProfileEdit {
                id: profile,
                label: "saved profile".into(),
                username: "user".into(),
                identity: Some(id),
            },
            Arc::new(|| true),
        )
        .await?;
    Ok(id)
}
#[test]
fn saved_alias_original_epoch_cancel_expiry_and_final_source_cas() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let id = default_agent(&f).await?;
        let source = f.source().await?;
        let capture = || {
            f.owner.begin_saved_alias_gesture_authorized(
                f.page.scope.clone(),
                "fixed-target",
                Arc::new(|| true),
            )
        };
        let (saved, selection) = capture()?.select_source(&f.store, &source)?;
        assert_eq!(selection.host_id(), f.host);
        assert_eq!(selection.identity_id(), id);
        let SavedSessionSelection::Agent(gesture) = saved.select_source(&source)? else {
            return Err("fixture.kind".into());
        };
        assert!(gesture.matches_selection(f.host, id));
        let action =
            gesture.begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        f.store
            .edit_ssh_agent_identity(
                f.store.load_ssh_agent_identities(Arc::new(|| true)).await?,
                AgentIdentityEdit::Update(
                    id,
                    pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                        "changed",
                        "user",
                        "ssh-ed25519",
                        &format!("SHA256:{}A", "B".repeat(42)),
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        assert!(action.prepare(&f.store, source).await.is_err());
        let source = f.source().await?;
        let old = capture()?;
        restore_same_dek(&f).await?;
        assert!(old.select_source(&f.store, &source).is_err());
        let mut expired = capture()?;
        expired.fixture_expire();
        assert!(expired.select_source(&f.store, &source).is_err());
        let alive = Arc::new(AtomicBool::new(true));
        let check = alive.clone();
        let original = f.owner.begin_saved_alias_gesture_authorized(
            f.page.scope.clone(),
            "fixed-target",
            Arc::new(move || check.load(Ordering::Acquire)),
        )?;
        alive.store(false, Ordering::Release);
        assert!(original.select_source(&f.store, &source).is_err());
        f.finish().await
    })
}
#[test]
fn cold_saved_alias_locked_unlock_separate_click_and_original_owner_page() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        default_agent(&f).await?;
        let source = f.source().await?;
        let intent = || {
            f.owner.begin_cold_saved_alias_intent_authorized(
                f.page.scope.clone(),
                "fixed-target",
                Arc::new(|| true),
            )
        };
        f.owner.lock()?;
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let original = intent()?;
        restore_same_dek(&f).await?;
        assert!(
            original
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| false))
                .is_err()
        );
        let foreign = PageVaultOwner::new(f.page.registry.clone());
        assert!(
            intent()?
                .confirm_authorized(&foreign, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let other = page()?;
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &other.scope, Arc::new(|| true))
                .is_err()
        );
        let mut expired = intent()?;
        expired.fixture_expire();
        assert!(
            expired
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        let clicked = intent()?.confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?;
        restore_same_dek(&f).await?;
        assert!(clicked.select_source(&f.store, &source).is_err());
        assert!(
            intent()?
                .confirm_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
                .select_source(&f.store, &f.source().await?)
                .is_ok()
        );
        drop(foreign);
        drop(other);
        f.finish().await
    })
}
