use super::*;
use crate::{
    database,
    ssh_trust::{TrustRecord, TrustTarget},
};
use std::sync::atomic::AtomicUsize;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn private() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn endpoint() -> std::result::Result<SshEndpoint, Box<dyn std::error::Error>> {
    Ok(SshEndpoint::new(
        "temporary",
        "temporary.example",
        22,
        "user",
    )?)
}
fn pin(host: &str) -> crate::Result<TrustRecord> {
    TrustRecord::new(TrustTarget::new(host, 22)?, "ssh-ed25519 AAAA".into())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn temporary_both_kinds_leave_host_graph_empty_and_clones_share_revocation() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let target = store.temporary_ssh_target(endpoint()?, Arc::new(|| true));
    let source = store
        .load_temporary_ssh_target_source(target.clone(), Arc::new(|| true))
        .await?;
    let key = store
        .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
        .await?;
    assert_eq!(source.hosts().records()[0].id(), target.id());
    assert!(source.hosts().records()[0].endpoint() == target.endpoint());
    assert_eq!(key.hosts().records()[0].id(), target.id());
    store
        .validate_ssh_target_source(source.clone(), Arc::new(|| true))
        .await?;
    store
        .validate_ssh_key_target_source(key.clone(), Arc::new(|| true))
        .await?;
    let c = database::open(&path)?;
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM app_state WHERE key IN (?1, ?2)",
            rusqlite::params![crate::ssh_hosts::KEY, crate::host_catalogue::KEY],
            |row| row.get::<_, i64>(0)
        )?,
        0
    );
    assert!(store.load_ssh_hosts().await?.records().is_empty());
    let guard = source.target_liveness();
    assert!(guard());
    target.clone().revoke();
    assert!(!guard());
    assert!(matches!(
        store
            .validate_ssh_target_source(source, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(matches!(
        store
            .validate_ssh_key_target_source(key, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    worker.shutdown().await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn foreign_expired_panicking_or_original_page_revoked_target_cannot_load() -> TestResult {
    let a = private()?;
    let b = private()?;
    let (store, worker) = StateStore::open(a.path().join("state.db")).await?;
    let (other, other_worker) = StateStore::open(b.path().join("state.db")).await?;
    let current = Arc::new(AtomicBool::new(true));
    let page = current.clone();
    let target =
        store.temporary_ssh_target(endpoint()?, Arc::new(move || page.load(Ordering::Acquire)));
    assert!(matches!(
        other
            .load_temporary_ssh_target_source(target.clone(), Arc::new(|| true))
            .await,
        Err(StoreError::IdentityConflict)
    ));
    assert!(matches!(
        other
            .load_temporary_ssh_key_target_source(target.clone(), Arc::new(|| true))
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let source = store
        .load_temporary_ssh_target_source(target, Arc::new(|| true))
        .await?;
    current.store(false, Ordering::Release);
    assert!(matches!(
        store
            .validate_ssh_target_source(source, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let mut expired = store.temporary_ssh_target(endpoint()?, Arc::new(|| true));
    Arc::get_mut(&mut expired.0).ok_or("not unique")?.deadline = Instant::now();
    assert!(matches!(
        store
            .load_temporary_ssh_key_target_source(expired, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let panic_target =
        store.temporary_ssh_target(endpoint()?, Arc::new(|| panic!("fixed scope panic")));
    assert!(matches!(
        store
            .load_temporary_ssh_target_source(panic_target, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    store.load_ssh_target_source(Arc::new(|| true)).await?;
    other_worker.shutdown().await?;
    worker.shutdown().await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn first_pin_binds_exact_temporary_target_and_final_revocation_rolls_back() -> TestResult {
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let target = store.temporary_ssh_target(endpoint()?, Arc::new(|| true));
    let source = store
        .load_temporary_ssh_target_source(target.clone(), Arc::new(|| true))
        .await?;
    assert!(matches!(
        store
            .approve_first_ssh_pin(
                source.clone(),
                target.id(),
                pin("other.example")?,
                Arc::new(|| true)
            )
            .await,
        Err(StoreError::HostTrustConflict)
    ));
    let count = AtomicUsize::new(0);
    let cancel = target.clone();
    let gate = Arc::new(move || {
        if count.fetch_add(1, Ordering::AcqRel) > 0 {
            cancel.revoke();
        }
        true
    });
    assert!(matches!(
        store
            .approve_first_ssh_pin(source, target.id(), pin("temporary.example")?, gate)
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(store.load_ssh_trust().await?.records().is_empty());
    let target = store.temporary_ssh_target(endpoint()?, Arc::new(|| true));
    let source = store
        .load_temporary_ssh_target_source(target.clone(), Arc::new(|| true))
        .await?;
    let approved = store
        .approve_first_ssh_pin(
            source,
            target.id(),
            pin("temporary.example")?,
            Arc::new(|| true),
        )
        .await?;
    assert_eq!(approved.trust().revision(), 1);
    assert_eq!(approved.hosts().records()[0].id(), target.id());
    store
        .validate_ssh_target_source(approved, Arc::new(|| true))
        .await?;
    assert!(store.load_ssh_hosts().await?.records().is_empty());
    worker.shutdown().await?;
    Ok(())
}
