use super::*;
#[tokio::test]
async fn jump_reference_worker_cas_reopen_cycle_and_dependent_delete_fail_closed() -> TestResult {
    let (dir, db, owner) = fixture()?;
    let before = ssh_hosts::load(&db, owner)?;
    let leaf_endpoint = before.records()[0].id();
    ssh_hosts::edit(
        &db,
        owner,
        &before,
        HostEdit::Create(SshEndpoint::new(
            "bastion",
            "jump.invalid",
            22,
            "fixed-user",
        )?),
        || true,
    )?;
    let raw: String = db.query_row(
        "SELECT value FROM app_state WHERE key=?",
        [crate::host_catalogue::KEY],
        |r| r.get(0),
    )?;
    let mut legacy: serde_json::Value = serde_json::from_str(&raw)?;
    legacy["version"] = serde_json::json!(1);
    for p in legacy["profiles"]
        .as_array_mut()
        .ok_or("fixture.profiles")?
    {
        p.as_object_mut().ok_or("fixture.profile")?.remove("via");
    }
    let v1 = serde_json::to_string(&legacy)?;
    db.execute(
        "UPDATE app_state SET value=? WHERE key=?",
        rusqlite::params![v1, crate::host_catalogue::KEY],
    )?;
    assert!(
        load(&db, owner)?
            .profiles()
            .all(|p| p.jump_profile_id().is_none())
    );
    let after_read: String = db.query_row(
        "SELECT value FROM app_state WHERE key=?",
        [crate::host_catalogue::KEY],
        |r| r.get(0),
    )?;
    assert_eq!(v1, after_read);
    let mut invalid = legacy.clone();
    invalid["profiles"][0]["via"] = serde_json::Value::Null;
    db.execute(
        "UPDATE app_state SET value=? WHERE key=?",
        rusqlite::params![serde_json::to_string(&invalid)?, crate::host_catalogue::KEY],
    )?;
    assert!(matches!(load(&db, owner), Err(StoreError::HostRecord)));
    invalid = legacy.clone();
    invalid["version"] = serde_json::json!(2);
    db.execute(
        "UPDATE app_state SET value=? WHERE key=?",
        rusqlite::params![serde_json::to_string(&invalid)?, crate::host_catalogue::KEY],
    )?;
    assert!(matches!(load(&db, owner), Err(StoreError::HostRecord)));
    db.execute(
        "UPDATE app_state SET value=? WHERE key=?",
        rusqlite::params![v1, crate::host_catalogue::KEY],
    )?;
    drop(db);
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let source = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let leaf = source
        .profiles()
        .find(|p| p.endpoint_id() == leaf_endpoint)
        .ok_or("fixture.leaf")?
        .metadata()
        .id();
    let via = source
        .profiles()
        .find(|p| p.endpoint_id() != leaf_endpoint)
        .ok_or("fixture.jump")?
        .metadata()
        .id();
    let via_endpoint = source
        .profiles()
        .find(|p| p.metadata().id() == via)
        .ok_or("fixture.jump")?
        .endpoint_id();
    let routed = store
        .edit_ssh_profile_jump(source.clone(), leaf, Some(via), Arc::new(|| true))
        .await?;
    assert_eq!(routed.hosts().revision(), source.hosts().revision() + 1);
    assert!(!routed.hosts().supports_direct_ssh(leaf_endpoint));
    assert!(routed.hosts().supports_direct_ssh(via_endpoint));
    assert!(source.passwords == routed.passwords && source.keys == routed.keys);
    assert!(
        source
            .hosts()
            .records()
            .iter()
            .zip(routed.hosts().records())
            .all(|(a, b)| a.id() == b.id() && a.endpoint() == b.endpoint())
    );
    assert!(matches!(
        store
            .edit_ssh_profile_jump(source, leaf, None, Arc::new(|| true))
            .await,
        Err(StoreError::HostConflict)
    ));
    assert!(matches!(
        store
            .edit_ssh_profile_with_jump(
                routed.clone(),
                ProfileEdit {
                    id: via,
                    label: "must roll back".into(),
                    username: "must-roll-back".into(),
                    identity: None,
                },
                Some(leaf),
                Arc::new(|| true)
            )
            .await,
        Err(StoreError::HostRecord)
    ));
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    assert!(matches!(
        store
            .edit_ssh_profile_jump(
                routed.clone(),
                leaf,
                None,
                Arc::new(move || count.fetch_add(1, Ordering::SeqCst) < 3)
            )
            .await,
        Err(StoreError::HostAuthorization)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    let unchanged = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    assert!(unchanged.hosts().source_matches(routed.hosts()));
    let renamed = store
        .edit_ssh_profile(
            unchanged,
            ProfileEdit {
                id: leaf,
                label: "independent routed login".into(),
                username: "fixed-user".into(),
                identity: None,
            },
            Arc::new(|| true),
        )
        .await?;
    assert_eq!(
        renamed
            .profiles()
            .find(|p| p.metadata().id() == leaf)
            .ok_or("fixture.leaf")?
            .jump_profile_id(),
        Some(via)
    );
    assert!(matches!(
        store
            .edit_ssh_hosts(
                renamed.hosts().clone(),
                HostEdit::Remove(via_endpoint),
                Arc::new(|| true)
            )
            .await,
        Err(StoreError::HostRecord)
    ));
    assert!(
        store
            .load_ssh_profiles_authorized(Arc::new(|| true))
            .await?
            .hosts()
            .source_matches(renamed.hosts())
    );
    drop(store);
    worker.shutdown().await?;
    let (store, worker) = StateStore::open(path).await?;
    let reopened = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    assert_eq!(
        reopened
            .hosts()
            .catalogue()
            .ok_or("fixture.graph")?
            .ssh_jump_chain(leaf)?
            .iter()
            .map(|p| p.metadata().id())
            .collect::<Vec<_>>(),
        vec![via]
    );
    let detached = store
        .edit_ssh_profile_with_jump(
            reopened,
            ProfileEdit {
                id: leaf,
                label: "direct after jump".into(),
                username: "explicit-user".into(),
                identity: None,
            },
            None,
            Arc::new(|| true),
        )
        .await?;
    assert!(detached.hosts().supports_direct_ssh(leaf_endpoint));
    assert_eq!(detached.hosts().revision(), renamed.hosts().revision() + 1);
    let direct = detached
        .profiles()
        .find(|p| p.metadata().id() == leaf)
        .ok_or("fixture.leaf")?;
    assert_eq!(direct.label(), "direct after jump");
    assert_eq!(direct.username(), "explicit-user");
    assert!(direct.jump_profile_id().is_none());
    assert_eq!(
        detached
            .hosts()
            .records()
            .iter()
            .find(|r| r.id() == leaf_endpoint)
            .ok_or("fixture.entrance")?
            .endpoint()
            .username(),
        "explicit-user"
    );
    let final_hosts = store
        .edit_ssh_hosts(
            detached.hosts().clone(),
            HostEdit::Remove(via_endpoint),
            Arc::new(|| true),
        )
        .await?;
    assert_eq!(final_hosts.records().len(), 1);
    drop(store);
    worker.shutdown().await?;
    Ok(())
}
