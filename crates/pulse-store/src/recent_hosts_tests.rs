use super::*;
use crate::ssh_hosts::HostEdit;
use pulse_core::ssh_endpoint::SshEndpoint;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[test]
fn bounded_mru_reopen_deleted_ids_and_corruption_never_fallback() -> TestResult {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let path = dir.path().join("state.sqlite3");
    let db = crate::database::open(&path)?;
    let owner = Uuid::new_v4();
    let mut hosts = crate::ssh_hosts::load(&db, owner)?;
    let mut ids = Vec::new();
    for index in 0..34 {
        hosts = crate::ssh_hosts::edit(
            &db,
            owner,
            &hosts,
            HostEdit::Create(SshEndpoint::new(
                &format!("host{index}"),
                "fixture.invalid",
                22,
                "user",
            )?),
            || true,
        )?;
        let id = hosts.records().last().ok_or("missing_host")?.id();
        ids.push(id);
        operation(&db, owner, Some(id), &|| true)?;
    }
    let recent = operation(&db, owner, None, &|| true)?;
    assert_eq!(recent.len(), 32);
    assert_eq!(recent[0].host, ids[33].to_string());
    assert!(!recent.iter().any(|r| r.host == ids[0].to_string()));
    let repeated = operation(&db, owner, Some(ids[8]), &|| true)?;
    assert_eq!(repeated.len(), 32);
    assert_eq!(repeated[0].host, ids[8].to_string());
    let missing = Uuid::now_v7();
    assert!(matches!(
        operation(&db, owner, Some(missing), &|| true),
        Err(StoreError::HostConflict)
    ));
    assert!(matches!(
        operation(&db, owner, None, &|| false),
        Err(StoreError::HostAuthorization)
    ));
    assert_eq!(load(&db)?.entries[0].host, ids[8].to_string());
    crate::ssh_hosts::edit(&db, owner, &hosts, HostEdit::Remove(ids[8]), || true)?;
    drop(db);
    let reopened = crate::database::open(&path)?;
    let visible = operation(&reopened, owner, None, &|| true)?;
    assert_eq!(visible.len(), 31);
    assert!(!visible.iter().any(|r| r.host == ids[8].to_string()));
    // Loading never rewrites local state; a deleted ID cannot select a replacement.
    assert_eq!(load(&reopened)?.entries.len(), 32);
    assert!(matches!(
        operation(&reopened, owner, Some(ids[8]), &|| true),
        Err(StoreError::HostConflict)
    ));
    reopened.execute(
        "UPDATE app_state SET value=?1 WHERE key=?2",
        rusqlite::params!["{corrupt", KEY],
    )?;
    assert!(matches!(
        operation(&reopened, owner, None, &|| true),
        Err(StoreError::HostRecord)
    ));
    assert!(matches!(
        operation(&reopened, owner, Some(ids[9]), &|| true),
        Err(StoreError::HostRecord)
    ));
    Ok(())
}
#[tokio::test]
async fn worker_records_without_mutating_endpoint_or_organization_revision() -> TestResult {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let (store, task) = StateStore::open(dir.path().join("state.sqlite3")).await?;
    let allow: Arc<dyn Fn() -> bool + Send + Sync> = Arc::new(|| true);
    assert!(store.load_recent_ssh_hosts(allow.clone()).await?.is_empty());
    let hosts = store
        .edit_ssh_hosts(
            store.load_ssh_hosts().await?,
            HostEdit::Create(SshEndpoint::new("test", "fixture.invalid", 22, "user")?),
            allow.clone(),
        )
        .await?;
    let id = hosts.records()[0].id();
    let organization = store
        .load_catalogue_organization(allow.clone())
        .await?
        .projection();
    store.record_recent_ssh_host(id).await?;
    assert_eq!(
        store.load_recent_ssh_hosts(allow.clone()).await?[0].host,
        id.to_string()
    );
    assert_eq!(store.load_ssh_hosts().await?.revision(), hosts.revision());
    assert_eq!(
        store.load_catalogue_organization(allow).await?.projection(),
        organization
    );
    drop(store);
    task.join().await?;
    Ok(())
}
