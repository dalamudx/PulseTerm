use super::*;
use crate::{
    database,
    ssh_hosts::HostEdit,
    ssh_trust::{TrustEdit, TrustRecord, TrustTarget},
};
use pulse_core::ssh_endpoint::SshEndpoint;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[path = "ssh_first_pin_tests.rs"]
mod first_pin_tests;

#[test]
fn trust_entity_same_revision_metadata_drift_rejects_source_without_key_change() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let before = load(&c, owner)?;
    let _pin = ssh_trust::edit(
        &c,
        owner,
        &before.trust,
        TrustEdit::Pin(TrustRecord::new(
            TrustTarget::new("fixed.example", 22)?,
            "ssh-ed25519 AAAA".into(),
        )?),
    )?;
    let source = load(&c, owner)?;
    c.execute("UPDATE known_hosts SET created_at=created_at-1", [])?;
    assert!(matches!(
        ssh_trust::edit(
            &c,
            owner,
            &source.trust,
            TrustEdit::Forget(TrustTarget::new("fixed.example", 22)?)
        ),
        Err(StoreError::HostTrustConflict)
    ));
    assert!(matches!(
        validate(&c, owner, &source, || true),
        Err(StoreError::IdentityConflict)
    ));
    Ok(())
}
fn private() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
#[test]
fn exact_source_rejects_same_revision_endpoint_drift_trust_update_and_foreign_empty() -> TestResult
{
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = load(&c, owner)?;
    validate(&c, owner, &empty, || true)?;
    assert!(matches!(
        validate(&c, Uuid::new_v4(), &empty, || true),
        Err(StoreError::IdentityConflict)
    ));
    let hosts = ssh_hosts::edit(
        &c,
        owner,
        &empty.hosts,
        HostEdit::Create(SshEndpoint::new("fixed-one", "fixed.example", 22, "user")?),
        || true,
    )?;
    let before = load(&c, owner)?;
    assert_eq!(before.hosts.records()[0].id(), hosts.records()[0].id());
    for key in [ssh_hosts::KEY, crate::host_catalogue::KEY] {
        c.execute(
            "UPDATE app_state SET value=replace(value,'fixed-one','fixed-tampered') WHERE key=?1",
            [key],
        )?;
    }
    let after = load(&c, owner)?;
    assert_eq!(before.hosts.revision(), after.hosts.revision());
    assert!(matches!(
        validate(&c, owner, &before, || true),
        Err(StoreError::IdentityConflict)
    ));
    let pin = TrustRecord::new(
        TrustTarget::new("fixed.example", 22)?,
        "ssh-ed25519 AAAA".into(),
    )?;
    let _pin = ssh_trust::edit(&c, owner, &after.trust, TrustEdit::Pin(pin))?;
    assert!(matches!(
        validate(&c, owner, &after, || true),
        Err(StoreError::IdentityConflict)
    ));
    let current = load(&c, owner)?;
    validate(&c, owner, &current, || true)?;
    assert_eq!(current.trust.records().len(), 1);
    assert!(matches!(
        validate(&c, owner, &current, || false),
        Err(StoreError::IdentityAuthorization)
    ));
    c.execute("UPDATE known_hosts SET key='not-json'", [])?;
    assert!(load(&c, owner).is_err());
    Ok(())
}
#[test]
fn same_wal_source_preserves_endpoint_and_pin_before_external_commit() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let c = database::open(&path)?;
    let other = database::open(&path)?;
    let owner = Uuid::new_v4();
    let empty = load(&c, owner)?;
    let host = ssh_hosts::edit(
        &c,
        owner,
        &empty.hosts,
        HostEdit::Create(SshEndpoint::new("fixed-one", "fixed.example", 22, "user")?),
        || true,
    )?;
    let tx = Transaction::new_unchecked(&c, TransactionBehavior::Deferred)
        .map_err(|_| StoreError::Database)?;
    let old_host = ssh_hosts::load(&tx, owner)?;
    let _new = ssh_hosts::edit(
        &other,
        owner,
        &host,
        HostEdit::Update(
            host.records()[0].id(),
            SshEndpoint::new("fixed-two", "new.example", 2222, "user")?,
        ),
        || true,
    )?;
    let trust = ssh_trust::load(&other, owner)?;
    let _pin = ssh_trust::edit(
        &other,
        owner,
        &trust,
        TrustEdit::Pin(TrustRecord::new(
            TrustTarget::new("new.example", 2222)?,
            "ssh-ed25519 AAAA".into(),
        )?),
    )?;
    let frozen = read(&tx, owner)?;
    assert_eq!(old_host.revision(), frozen.hosts.revision());
    assert!(frozen.trust.records().is_empty());
    tx.rollback()?;
    let new = load(&c, owner)?;
    assert_ne!(new.hosts.revision(), frozen.hosts.revision());
    assert_eq!(new.trust.records().len(), 1);
    assert!(matches!(
        validate(&c, owner, &frozen, || true),
        Err(StoreError::IdentityConflict)
    ));
    Ok(())
}
#[tokio::test]
async fn typed_worker_foreign_instance_and_denied_validation_never_publish() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let (foreign, other) = StateStore::open(path).await?;
    let source = store.load_ssh_target_source(Arc::new(|| true)).await?;
    assert!(matches!(
        foreign
            .validate_ssh_target_source(source.clone(), Arc::new(|| true))
            .await,
        Err(StoreError::IdentityConflict)
    ));
    store
        .validate_ssh_target_source(source.clone(), Arc::new(|| true))
        .await?;
    assert!(matches!(
        store
            .validate_ssh_target_source(source, Arc::new(|| false))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    drop(foreign);
    other.join().await?;
    drop(store);
    worker.join().await?;
    Ok(())
}
