use crate::{
    Request, StateStore, StoreError, dispatch,
    ssh_trust::{self, TrustEdit, TrustRecord, TrustTarget},
};
use tokio::sync::oneshot;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
const K1: &str = "ssh-ed25519 AAAA"; // storage shape only; SSH adapter must reject this as a public key
const K2: &str = "ssh-ed25519 BBBB";
fn directory() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}
fn target(name: &str) -> crate::Result<TrustTarget> {
    TrustTarget::new(name, 2222)
}
fn pin(name: &str, key: &str) -> crate::Result<TrustEdit> {
    Ok(TrustEdit::Pin(TrustRecord::new(
        target(name)?,
        key.to_owned(),
    )?))
}
#[tokio::test]
async fn independent_owners_empty_receipts_cas_reopen_and_deletion() -> TestResult {
    let d = directory()?;
    let path = d.path().join("state.db");
    let other = d.path().join("other.db");
    let (a, task_a) = StateStore::open(path.clone()).await?;
    let (b, task_b) = StateStore::open(path.clone()).await?;
    let (foreign, task_foreign) = StateStore::open(other).await?;
    let empty_a = a.load_ssh_trust().await?;
    let empty_b = b.load_ssh_trust().await?;
    let empty_foreign = foreign.load_ssh_trust().await?;
    assert_eq!(empty_a.revision(), 0);
    assert!(empty_a.records().is_empty());
    assert!(matches!(
        b.edit_ssh_trust(empty_a.clone(), pin("alpha.example", K1)?)
            .await,
        Err(StoreError::HostTrustConflict)
    ));
    assert!(matches!(
        a.edit_ssh_trust(empty_foreign, pin("alpha.example", K1)?)
            .await,
        Err(StoreError::HostTrustConflict)
    ));
    let committed = a
        .edit_ssh_trust(empty_a.clone(), pin("alpha.example", K1)?)
        .await?;
    assert_eq!(committed.revision(), 1);
    assert_eq!(committed.records()[0].target().host(), "alpha.example");
    assert_eq!(committed.records()[0].key(), K1);
    assert!(matches!(
        a.edit_ssh_trust(empty_a, pin("alpha.example", K2)?).await,
        Err(StoreError::HostTrustConflict)
    ));
    assert!(matches!(
        b.edit_ssh_trust(empty_b, pin("beta.example", K2)?).await,
        Err(StoreError::HostTrustConflict)
    ));
    let fresh_b = b.load_ssh_trust().await?;
    let merged = b.edit_ssh_trust(fresh_b, pin("beta.example", K2)?).await?;
    assert_eq!(merged.revision(), 2);
    assert_eq!(merged.records().len(), 2);
    let forgot = b
        .edit_ssh_trust(merged, TrustEdit::Forget(target("alpha.example")?))
        .await?;
    assert_eq!(forgot.revision(), 3);
    assert_eq!(forgot.records().len(), 1);
    assert_eq!(forgot.records()[0].target().host(), "beta.example");
    assert!(matches!(
        a.edit_ssh_trust(committed, pin("alpha.example", K2)?).await,
        Err(StoreError::HostTrustConflict)
    ));
    task_a.shutdown().await?;
    task_b.shutdown().await?;
    task_foreign.shutdown().await?;
    let (reopened, task) = StateStore::open(path).await?;
    let receipt = reopened.load_ssh_trust().await?;
    assert_eq!(receipt.revision(), 3);
    assert_eq!(receipt.records()[0].key(), K2);
    task.shutdown().await?;
    Ok(())
}
#[test]
fn malformed_and_oversized_rows_do_not_become_empty_or_rewrite_other_state() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    crate::database::edit(
        &c,
        crate::profiles::AutoProfileEdit::Suppress(crate::profiles::AutomaticShell::Bash),
    )?;
    let owner = Uuid::new_v4();
    for text in [
        String::from("{"),
        String::from(r#"{"schemaVersion":2,"revision":1,"pins":[]}"#),
        String::from(r#"{"schemaVersion":1,"revision":0,"pins":[]}"#),
        String::from(r#"{"schemaVersion":1,"revision":1,"pins":[],"unknown":0}"#),
        String::from(
            r#"{"schemaVersion":1,"revision":1,"pins":[{"host":"UPPER.example","port":2222,"key":"ssh-ed25519 AAAA"}]}"#,
        ),
        String::from(
            r#"{"schemaVersion":1,"revision":1,"pins":[{"host":"lower.example","port":2222,"key":"ssh-ed25519 invalid!"}]}"#,
        ),
        String::from(
            r#"{"schemaVersion":1,"revision":1,"pins":[{"host":"lower.example","port":2222,"key":"ssh-ed25519 AAAA"},{"host":"lower.example","port":2222,"key":"ssh-ed25519 AAAA"}]}"#,
        ),
        "x".repeat(65_537),
    ] {
        c.execute("INSERT INTO app_state(key,value) VALUES ('ssh.trusted_hosts.v1',?1) ON CONFLICT(key) DO UPDATE SET value=excluded.value", [text])?;
        assert!(matches!(
            ssh_trust::load(&c, owner),
            Err(StoreError::HostTrustRecord)
        ));
        assert!(
            crate::database::load(&c)?
                .deleted()
                .any(|shell| shell == crate::profiles::AutomaticShell::Bash)
        );
    }
    Ok(())
}
#[test]
fn record_limit_rejects_new_host_without_rolling_back_existing() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let mut current = ssh_trust::load(&c, owner)?;
    for n in 0..128 {
        current = ssh_trust::edit(&c, owner, &current, pin(&format!("h{n}.example"), K1)?)?;
    }
    assert_eq!(current.records().len(), 128);
    assert!(matches!(
        ssh_trust::edit(&c, owner, &current, pin("overflow.example", K1)?),
        Err(StoreError::HostTrustRecord)
    ));
    let after = ssh_trust::load(&c, owner)?;
    assert_eq!(after.revision(), 128);
    assert_eq!(after.records().len(), 128);
    Ok(())
}
#[test]
fn exhausted_revision_and_closed_reply_cannot_commit() -> TestResult {
    let d = directory()?;
    let c = crate::database::open(&d.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = ssh_trust::load(&c, owner)?;
    let active = ssh_trust::edit(&c, owner, &empty, pin("seed.example", K1)?)?;
    let _deleted = ssh_trust::edit(
        &c,
        owner,
        &active,
        TrustEdit::Forget(target("seed.example")?),
    )?;
    c.execute("UPDATE known_hosts_state SET revision=?1", [i64::MAX])?;
    let before = ssh_trust::load(&c, owner)?;
    assert!(matches!(
        ssh_trust::edit(&c, owner, &before, pin("alpha.example", K1)?),
        Err(StoreError::HostTrustRevision)
    ));
    let (reply, result) = oneshot::channel();
    drop(result);
    dispatch(
        &c,
        Request::HostTrustEdit(owner, before, pin("beta.example", K1)?, reply),
    );
    let after = ssh_trust::load(&c, owner)?;
    assert_eq!(after.revision(), i64::MAX as u64);
    assert!(after.records().is_empty());
    Ok(())
}
