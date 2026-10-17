use super::*;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture()
-> std::result::Result<(tempfile::TempDir, Connection, Uuid), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let db = crate::database::open(&dir.path().join("state.db"))?;
    Ok((dir, db, Uuid::new_v4()))
}
fn value(label: &str) -> std::result::Result<SshAgentIdentity, Box<dyn std::error::Error>> {
    Ok(SshAgentIdentity::new(
        label,
        "user",
        "ssh-ed25519",
        &format!("SHA256:{}", "A".repeat(43)),
    )?)
}
#[test]
fn agent_directory_reads_are_zero_write_and_revoked_first_save_leaves_no_schema() -> TestResult {
    let (_dir, db, owner) = fixture()?;
    let empty = load(&db, owner)?;
    assert_eq!(empty.revision(), 0);
    assert_eq!(empty.records().count(), 0);
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        edit(
            &db,
            owner,
            &empty,
            AgentIdentityEdit::Create(value("rejected")?),
            || calls.fetch_add(1, Ordering::SeqCst) == 0
        ),
        Err(StoreError::IdentityAuthorization)
    ));
    let count:i64=db.query_row("SELECT count(*) FROM sqlite_master WHERE name IN('agent_identity_catalog','agent_identities')",[],|r|r.get(0))?;
    assert_eq!(count, 0);
    assert!(same_metadata(&empty, &load(&db, owner)?));
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        edit(
            &db,
            Uuid::new_v4(),
            &empty,
            AgentIdentityEdit::Create(value("foreign")?),
            || {
                calls.fetch_add(1, Ordering::SeqCst);
                true
            }
        ),
        Err(StoreError::IdentityConflict)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}
#[test]
fn agent_directory_same_revision_changes_tombstones_and_reopen_keep_complete_metadata() -> TestResult
{
    let (dir, db, owner) = fixture()?;
    db.execute(
        "INSERT INTO app_state(key,value) VALUES('agent.test.invariant','unchanged')",
        [],
    )?;
    let empty = load(&db, owner)?;
    let saved = edit(
        &db,
        owner,
        &empty,
        AgentIdentityEdit::Create(value("selected key")?),
        || true,
    )?;
    let row = saved.records().next().ok_or("fixture.agent")?;
    let id = row.id();
    let created = row.created_at();
    assert_eq!(id.get_version_num(), 7);
    db.execute(
        "UPDATE agent_identities SET label='same revision replacement' WHERE id=?1",
        [id.to_string()],
    )?;
    assert!(matches!(
        edit(&db, owner, &saved, AgentIdentityEdit::Remove(id), || true),
        Err(StoreError::IdentityConflict)
    ));
    db.execute(
        "UPDATE agent_identities SET label='selected key' WHERE id=?1",
        [id.to_string()],
    )?;
    let removed = edit(&db, owner, &saved, AgentIdentityEdit::Remove(id), || true)?;
    assert_eq!(removed.records().count(), 0);
    assert_eq!(removed.revision(), 2);
    assert!(matches!(
        edit(
            &db,
            owner,
            &saved,
            AgentIdentityEdit::Update(id, value("stale")?),
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    assert!(matches!(
        edit(
            &db,
            owner,
            &removed,
            AgentIdentityEdit::Update(id, value("deleted")?),
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    let invariant: String = db.query_row(
        "SELECT value FROM app_state WHERE key='agent.test.invariant'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(invariant, "unchanged");
    db.close().map_err(|(_, e)| e)?;
    let reopened = crate::database::open(&dir.path().join("state.db"))?;
    let source = load(&reopened, owner)?;
    assert!(same_metadata(&source, &removed));
    let deleted = source.rows.first().ok_or("fixture.tombstone")?;
    assert_eq!(deleted.id, id);
    assert_eq!(deleted.created_at, created);
    assert!(deleted.deleted);
    assert_eq!(
        deleted.value.fingerprint(),
        format!("SHA256:{}", "A".repeat(43))
    );
    Ok(())
}

#[tokio::test]
async fn agent_worker_cancelled_waiter_keeps_callback_credit_and_rolls_back_without_schema_side_effects()
-> TestResult {
    let (dir, db, _owner) = fixture()?;
    db.close().map_err(|(_, e)| e)?;
    let (store, worker) = crate::StateStore::open(dir.path().join("state.db")).await?;
    let initial = store.load_ssh_agent_identities(Arc::new(|| true)).await?;
    let saved = store
        .edit_ssh_agent_identity(
            initial,
            AgentIdentityEdit::Create(value("explicit selection")?),
            Arc::new(|| true),
        )
        .await?;
    let identity = saved.records().next().ok_or("fixture.agent")?.id();
    struct Credit(Arc<std::sync::atomic::AtomicBool>);
    impl Drop for Credit {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }
    let exited = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let credit = Credit(exited.clone());
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = std::sync::Mutex::new(Some(entered));
    let (release, wait) = std::sync::mpsc::channel();
    let wait = std::sync::Mutex::new(wait);
    let authorize: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(move || {
        let _held = &credit;
        if let Some(tx) = entered.lock().ok().and_then(|mut value| value.take()) {
            let _sent = tx.send(());
            return wait
                .lock()
                .ok()
                .is_some_and(|receiver| receiver.recv().is_ok());
        }
        true
    });
    let client = store.clone();
    let expected = saved.clone();
    let change = AgentIdentityEdit::Update(identity, value("cancelled replacement")?);
    let caller = tokio::spawn(async move {
        client
            .edit_ssh_agent_identity(expected, change, authorize)
            .await
    });
    started.await?;
    caller.abort();
    assert!(caller.await.is_err());
    assert!(!exited.load(Ordering::SeqCst));
    release.send(())?;
    let after = store.load_ssh_agent_identities(Arc::new(|| true)).await?;
    assert!(same_metadata(&saved, &after));
    assert!(exited.load(Ordering::SeqCst));
    assert!(matches!(
        store
            .load_ssh_agent_identities(Arc::new(|| panic!("authorization rejected")))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(same_metadata(
        &saved,
        &store.load_ssh_agent_identities(Arc::new(|| true)).await?
    ));
    drop(store);
    worker.join().await?;
    let reopened = crate::database::open(&dir.path().join("state.db"))?;
    assert!(same_metadata(&saved, &load(&reopened, saved.owner)?));
    Ok(())
}
