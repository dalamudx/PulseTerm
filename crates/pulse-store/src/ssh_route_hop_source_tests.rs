use super::*;

#[tokio::test]
async fn derived_hops_keep_exact_identity_kind_index_and_store_provenance() -> TestResult {
    let f = fixture()?;
    let before = app_state(&f.db)?;
    let (store, worker) = StateStore::open(f.dir.path().join("state.db")).await?;
    let route = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    assert!(route.hop_source(3).is_err());
    assert!(route.hop_source(usize::MAX).is_err());
    assert!(
        !route
            .target_source()
            .password_source()
            .supports_password_selection(f.entrances[2], f.password)
    );
    for i in 0..3 {
        let child = route.hop_source(i)?;
        for j in 0..3 {
            assert_eq!(
                child
                    .password_source()
                    .supports_password_selection(f.entrances[j], f.password),
                i == j && i != 1
            );
            assert_eq!(
                child.supports_private_key_selection(f.entrances[j], f.key),
                i == j && i == 1
            );
            assert!(
                !child
                    .password_source()
                    .supports_password_selection(f.entrances[j], f.key)
            );
            assert!(!child.supports_private_key_selection(f.entrances[j], f.password));
        }
        store
            .validate_ssh_target_source(child.password_source().clone(), Arc::new(|| true))
            .await?;
        store
            .validate_ssh_key_target_source(child, Arc::new(|| true))
            .await?;
    }
    let stale = route.hop_source(2)?;
    drop(store);
    worker.shutdown().await?;
    let (other, worker) = StateStore::open(f.dir.path().join("state.db")).await?;
    assert!(
        other
            .validate_ssh_target_source(stale.password_source().clone(), Arc::new(|| true))
            .await
            .is_err()
    );
    assert!(
        other
            .validate_ssh_key_target_source(stale, Arc::new(|| true))
            .await
            .is_err()
    );
    drop(other);
    worker.shutdown().await?;
    assert_eq!(app_state(&f.db)?, before);
    Ok(())
}

#[tokio::test]
async fn password_hop_rechecks_private_key_metadata_inside_its_existing_transaction() -> TestResult
{
    let f = fixture()?;
    let (store, worker) = StateStore::open(f.dir.path().join("state.db")).await?;
    let route = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    let revision = route.target_source().identities().revision();
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["same-revision replacement", f.key.to_string()],
    )?;
    assert_eq!(
        ssh_key_identities::load(&f.db, f.owner)?.revision(),
        revision
    );
    for i in 0..3 {
        let child = route.hop_source(i)?;
        assert!(
            store
                .validate_ssh_target_source(child.password_source().clone(), Arc::new(|| true))
                .await
                .is_err()
        );
        assert!(
            store
                .validate_ssh_key_target_source(child, Arc::new(|| true))
                .await
                .is_err()
        );
    }
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["route-key", f.key.to_string()],
    )?;
    store
        .validate_ssh_target_source(
            route.hop_source(2)?.password_source().clone(),
            Arc::new(|| true),
        )
        .await?;
    drop(store);
    worker.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn first_pin_is_exact_hop_final_revoke_rolls_back_and_reply_keeps_route_binding() -> TestResult
{
    let f = fixture()?;
    let (store, worker) = StateStore::open(f.dir.path().join("state.db")).await?;
    let route = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    let leaf = route.hop_source(2)?.password_source().clone();
    let pin = |host: &str| {
        crate::ssh_trust::TrustRecord::new(
            crate::ssh_trust::TrustTarget::new(host, 22)?,
            "ssh-ed25519 AAAA".into(),
        )
    };
    let before = store.load_ssh_trust().await?.revision();
    assert!(
        store
            .approve_first_ssh_pin(
                leaf.clone(),
                f.entrances[0],
                pin("outer.invalid")?,
                Arc::new(|| true)
            )
            .await
            .is_err()
    );
    let calls = AtomicUsize::new(0);
    assert!(
        store
            .approve_first_ssh_pin(
                leaf.clone(),
                f.entrances[2],
                pin("destination.invalid")?,
                Arc::new(move || calls.fetch_add(1, Ordering::SeqCst) == 0)
            )
            .await
            .is_err()
    );
    assert_eq!(store.load_ssh_trust().await?.revision(), before);
    let updated = store
        .approve_first_ssh_pin(
            leaf.clone(),
            f.entrances[2],
            pin("destination.invalid")?,
            Arc::new(|| true),
        )
        .await?;
    assert_eq!(updated.trust().revision(), before + 1);
    assert!(updated.supports_password_selection(f.entrances[2], f.password));
    assert!(!updated.supports_password_selection(f.entrances[0], f.password));
    assert!(route.after_first_pin(0, &updated).is_err());
    assert!(route.after_first_pin(2, &leaf).is_err());
    let successor = route.after_first_pin(2, &updated)?;
    store
        .validate_ssh_key_target_source(successor.hop_source(0)?, Arc::new(|| true))
        .await?;
    assert!(successor.after_first_pin(2, &updated).is_err());
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["metadata changed with pin", f.key.to_string()],
    )?;
    let other = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    assert!(
        route
            .after_first_pin(2, other.hop_source(2)?.password_source())
            .is_err()
    );
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["route-key", f.key.to_string()],
    )?;
    store
        .validate_ssh_target_source(updated, Arc::new(|| true))
        .await?;
    assert!(
        store
            .validate_ssh_target_source(leaf, Arc::new(|| true))
            .await
            .is_err()
    );
    assert!(
        store
            .validate_ssh_key_target_source(route.hop_source(0)?, Arc::new(|| true))
            .await
            .is_err()
    );
    drop(store);
    worker.shutdown().await?;
    Ok(())
}
