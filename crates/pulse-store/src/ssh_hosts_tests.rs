use crate::{
    Request, StateStore, StoreError, dispatch,
    ssh_hosts::{self, HostEdit},
};
use pulse_core::ssh_endpoint::SshEndpoint;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn directory() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}
fn endpoint(label: &str) -> Result<SshEndpoint, pulse_core::ssh_endpoint::InvalidSshEndpoint> {
    SshEndpoint::new(label, "Example.COM", 2222, "user@example.com")
}
fn allow() -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(|| true)
}
#[tokio::test]
async fn catalogue_instance_cas_reopen_and_deleted_identity_never_resurrects() -> TestResult {
    let d = directory()?;
    let path = d.path().join("state.db");
    let (a, ta) = StateStore::open(path.clone()).await?;
    let (b, tb) = StateStore::open(path.clone()).await?;
    let empty = a.load_ssh_hosts().await?;
    let b_empty = b.load_ssh_hosts().await?;
    assert!(matches!(
        b.edit_ssh_hosts(
            empty.clone(),
            HostEdit::Create(endpoint("错误来源")?),
            allow()
        )
        .await,
        Err(StoreError::HostConflict)
    ));
    let first = a
        .edit_ssh_hosts(
            empty.clone(),
            HostEdit::Create(endpoint("开发机")?),
            allow(),
        )
        .await?;
    let id = first.records()[0].id();
    assert_eq!(id.get_version_num(), 7);
    assert_eq!(first.records()[0].endpoint().target().host(), "example.com");
    assert_eq!(first.revision(), 1);
    assert!(matches!(
        b.edit_ssh_hosts(b_empty, HostEdit::Create(endpoint("旧快照")?), allow())
            .await,
        Err(StoreError::HostConflict)
    ));
    let second = a
        .edit_ssh_hosts(
            first.clone(),
            HostEdit::Update(id, endpoint("改名")?),
            allow(),
        )
        .await?;
    assert!(matches!(
        a.edit_ssh_hosts(first, HostEdit::Remove(id), allow()).await,
        Err(StoreError::HostConflict)
    ));
    let removed = a
        .edit_ssh_hosts(second, HostEdit::Remove(id), allow())
        .await?;
    assert_eq!(removed.revision(), 3);
    assert!(removed.records().is_empty());
    assert!(matches!(
        a.edit_ssh_hosts(
            removed.clone(),
            HostEdit::Update(id, endpoint("不能复活")?),
            allow()
        )
        .await,
        Err(StoreError::HostConflict)
    ));
    let new = a
        .edit_ssh_hosts(removed, HostEdit::Create(endpoint("新记录")?), allow())
        .await?;
    assert_ne!(new.records()[0].id(), id);
    let new_id = new.records()[0].id();
    drop(a);
    drop(b);
    ta.join().await?;
    tb.join().await?;
    let (c, tc) = StateStore::open(path).await?;
    let reopened = c.load_ssh_hosts().await?;
    assert_eq!(reopened.revision(), 4);
    assert_eq!(reopened.records()[0].id(), new_id);
    assert_eq!(reopened.records()[0].endpoint().label(), "新记录");
    assert!(matches!(
        c.edit_ssh_hosts(empty, HostEdit::Remove(new_id), allow())
            .await,
        Err(StoreError::HostConflict)
    ));
    drop(c);
    tc.join().await?;
    Ok(())
}
#[test]
fn malformed_future_noncanonical_duplicate_and_oversize_source_never_becomes_empty() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = ssh_hosts::load(&c, owner)?;
    let id = Uuid::now_v7();
    let row = serde_json::json!({"id":id.to_string(),"label":"主机","host":"example.com","port":22,"username":"user"});
    let mut invalid_host = row.clone();
    invalid_host["host"] = serde_json::json!("EXAMPLE.COM");
    let mut invalid_id = row.clone();
    invalid_id["id"] = serde_json::json!(Uuid::new_v4().to_string());
    let mut extra = row.clone();
    extra["password"] = serde_json::json!("not-supported");
    let docs = vec![
        "broken".to_owned(),
        serde_json::json!({"schemaVersion":2,"revision":1,"hosts":[]}).to_string(),
        serde_json::json!({"schemaVersion":1,"revision":0,"hosts":[]}).to_string(),
        serde_json::json!({"schemaVersion":1,"revision":1,"hosts":[row.clone(),row]}).to_string(),
        serde_json::json!({"schemaVersion":1,"revision":1,"hosts":[invalid_host]}).to_string(),
        serde_json::json!({"schemaVersion":1,"revision":1,"hosts":[invalid_id]}).to_string(),
        serde_json::json!({"schemaVersion":1,"revision":1,"hosts":[extra]}).to_string(),
        "x".repeat(262145),
    ];
    for text in docs {
        c.execute(
            "INSERT OR REPLACE INTO app_state(key,value) VALUES('ssh.endpoints.v1',?1)",
            [&text],
        )?;
        assert!(matches!(
            ssh_hosts::load(&c, owner),
            Err(StoreError::HostRecord)
        ));
        assert!(matches!(
            ssh_hosts::edit(
                &c,
                owner,
                &empty,
                HostEdit::Create(endpoint("不能覆盖")?),
                || true
            ),
            Err(StoreError::HostRecord)
        ));
        let after: String = c.query_row(
            "SELECT value FROM app_state WHERE key='ssh.endpoints.v1'",
            [],
            |r| r.get(0),
        )?;
        assert_eq!(after, text);
    }
    Ok(())
}
#[test]
fn revocation_before_prepare_write_or_commit_rolls_back_all_metadata() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = ssh_hosts::load(&c, owner)?;
    for denied in 1..=3 {
        let calls = AtomicUsize::new(0);
        assert!(matches!(
            ssh_hosts::edit(
                &c,
                owner,
                &empty,
                HostEdit::Create(endpoint("撤销")?),
                || calls.fetch_add(1, Ordering::SeqCst) + 1 != denied
            ),
            Err(StoreError::HostAuthorization)
        ));
        assert_eq!(calls.load(Ordering::SeqCst), denied);
        let after = ssh_hosts::load(&c, owner)?;
        assert_eq!(after.revision(), 0);
        assert!(after.records().is_empty());
    }
    let (receipt, result) = tokio::sync::oneshot::channel();
    drop(result);
    let calls = Arc::new(AtomicUsize::new(0));
    let count = calls.clone();
    dispatch(
        &c,
        Request::HostsEdit(
            owner,
            empty,
            HostEdit::Create(endpoint("已关闭回执")?),
            Arc::new(move || {
                count.fetch_add(1, Ordering::SeqCst);
                true
            }),
            receipt,
        ),
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(ssh_hosts::load(&c, owner)?.revision(), 0);
    Ok(())
}
#[test]
fn limits_revision_exhaustion_and_other_rows_are_preserved() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    c.execute(
        "INSERT INTO app_state(key,value) VALUES('sentinel','keep')",
        [],
    )?;
    let owner = Uuid::new_v4();
    let mut current = ssh_hosts::load(&c, owner)?;
    for _ in 0..256 {
        current = ssh_hosts::edit(
            &c,
            owner,
            &current,
            HostEdit::Create(endpoint("主机")?),
            || true,
        )?;
    }
    assert!(matches!(
        ssh_hosts::edit(
            &c,
            owner,
            &current,
            HostEdit::Create(endpoint("超限")?),
            || true
        ),
        Err(StoreError::HostRecord)
    ));
    assert_eq!(ssh_hosts::load(&c, owner)?.revision(), 256);
    // Build a consistent empty catalogue at the maximum revision in both representations.
    let tx = c.unchecked_transaction()?;
    let raw: String = tx.query_row(
        "SELECT value FROM app_state WHERE key=?1",
        [crate::host_catalogue::KEY],
        |row| row.get(0),
    )?;
    let mut graph: serde_json::Value = serde_json::from_str(&raw)?;
    graph["legacyRevision"] = serde_json::json!(i64::MAX);
    for kind in ["hosts", "endpoints", "profiles"] {
        for row in graph[kind].as_array_mut().ok_or("missing graph records")? {
            row["metadata"]["deleted"] = serde_json::json!(true);
        }
    }
    tx.execute(
        "UPDATE app_state SET value=?1 WHERE key='ssh.endpoints.v1'",
        [serde_json::json!({"schemaVersion":1,"revision":i64::MAX,"hosts":[]}).to_string()],
    )?;
    tx.execute(
        "UPDATE app_state SET value=?1 WHERE key=?2",
        rusqlite::params![graph.to_string(), crate::host_catalogue::KEY],
    )?;
    tx.commit()?;
    let max = ssh_hosts::load(&c, owner)?;
    assert!(matches!(
        ssh_hosts::edit(
            &c,
            owner,
            &max,
            HostEdit::Create(endpoint("耗尽")?),
            || true
        ),
        Err(StoreError::HostRevision)
    ));
    assert_eq!(ssh_hosts::load(&c, owner)?.revision(), i64::MAX as u64);
    let untouched: String = c.query_row(
        "SELECT value FROM app_state WHERE key='sentinel'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(untouched, "keep");
    Ok(())
}
#[tokio::test]
async fn panicked_admission_is_denied_and_shared_worker_remains_usable() -> TestResult {
    let d = directory()?;
    let (store, task) = StateStore::open(d.path().join("state.db")).await?;
    let empty = store.load_ssh_hosts().await?;
    assert!(matches!(
        store
            .edit_ssh_hosts(
                empty,
                HostEdit::Create(endpoint("panic")?),
                Arc::new(|| panic!("fixed fixture"))
            )
            .await,
        Err(StoreError::HostAuthorization)
    ));
    assert_eq!(store.load_ssh_hosts().await?.revision(), 0);
    store.load_auto_profiles().await?;
    task.cancel();
    task.join().await?;
    assert!(matches!(
        store.load_ssh_hosts().await,
        Err(StoreError::Closed)
    ));
    Ok(())
}

#[tokio::test]
async fn batch_removal_is_one_revision_and_invalid_or_revoked_batch_never_partially_writes()
-> TestResult {
    let d = directory()?;
    let path = d.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let mut snapshot = store.load_ssh_hosts().await?;
    for name in ["first", "second", "survivor"] {
        snapshot = store
            .edit_ssh_hosts(snapshot, HostEdit::Create(endpoint(name)?), allow())
            .await?;
    }
    let ids: Vec<_> = snapshot.records().iter().take(2).map(|r| r.id()).collect();
    for invalid in [
        vec![],
        vec![ids[0], ids[0]],
        vec![ids[0], Uuid::now_v7()],
        vec![Uuid::new_v4()],
        vec![ids[0]; 257],
    ] {
        assert!(
            store
                .edit_ssh_hosts(snapshot.clone(), HostEdit::RemoveMany(invalid), allow())
                .await
                .is_err()
        );
        assert_eq!(store.load_ssh_hosts().await?.revision(), 3);
        assert_eq!(store.load_ssh_hosts().await?.records().len(), 3);
    }
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        store
            .edit_ssh_hosts(
                snapshot.clone(),
                HostEdit::RemoveMany(ids.clone()),
                Arc::new(move || calls.fetch_add(1, Ordering::SeqCst) < 2)
            )
            .await,
        Err(StoreError::HostAuthorization)
    ));
    assert_eq!(store.load_ssh_hosts().await?.records().len(), 3);
    let saved = store
        .edit_ssh_hosts(snapshot.clone(), HostEdit::RemoveMany(ids.clone()), allow())
        .await?;
    assert_eq!(saved.revision(), 4);
    assert_eq!(saved.records().len(), 1);
    assert!(matches!(
        store
            .edit_ssh_hosts(snapshot, HostEdit::RemoveMany(ids), allow())
            .await,
        Err(StoreError::HostConflict)
    ));
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path).await?;
    assert_eq!(store.load_ssh_hosts().await?.records().len(), 1);
    assert_eq!(store.load_ssh_hosts().await?.revision(), 4);
    drop(store);
    worker.join().await?;
    Ok(())
}
