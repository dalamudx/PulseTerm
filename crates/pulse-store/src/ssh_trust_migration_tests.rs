use super::*;
use crate::database;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn catalog(c: &Connection, stamp: Hlc) -> crate::Result<()> {
    c.execute(
        "UPDATE known_hosts_state SET revision=1,node_id=?1,clock=?2",
        rusqlite::params![stamp.node().to_string(), stamp.encode()],
    )
    .map_err(|_| StoreError::Database)?;
    Ok(())
}
fn row(c: &Connection, host: &str, key: &str, ordinal: i64, stamp: Hlc) -> crate::Result<()> {
    c.execute("INSERT INTO known_hosts(id,created_at,hlc,deleted,host,port,key,ordinal) VALUES (?1,1,?2,0,?3,22,?4,?5)",rusqlite::params![Uuid::now_v7().to_string(),stamp.encode(),host,key,ordinal]).map_err(|_|StoreError::Database)?;
    Ok(())
}
#[test]
fn trust_entity_id_hlc_tombstone_explicit_restore_and_same_revision_source_metadata() -> TestResult
{
    let dir = private()?;
    let path = dir.path().join("state.db");
    let c = database::open(&path)?;
    let owner = Uuid::new_v4();
    let empty = super::super::load(&c, owner)?;
    let target = TrustTarget::new("fixed.example", 22)?;
    let pin = || TrustRecord::new(target.clone(), "ssh-ed25519 AAAA".into());
    let active = super::super::edit(&c, owner, &empty, TrustEdit::Pin(pin()?))?;
    let original = active.rows[0].clone();
    assert_eq!(original.id.get_version_num(), 7);
    assert!(original.created_at > 0);
    assert!(!original.deleted);
    let deleted = super::super::edit(&c, owner, &active, TrustEdit::Forget(target.clone()))?;
    assert!(deleted.records().is_empty());
    assert_eq!(deleted.rows.len(), 1);
    assert_eq!(deleted.rows[0].id, original.id);
    assert_eq!(deleted.rows[0].created_at, original.created_at);
    assert!(deleted.rows[0].stamp > original.stamp);
    assert!(deleted.rows[0].deleted);
    assert!(deleted.rows[0].key.is_none());
    let reopened = Connection::open(&path)?;
    let frozen = load(&reopened, owner)?;
    assert!(frozen.rows == deleted.rows);
    let restored = super::super::edit(&c, owner, &deleted, TrustEdit::Pin(pin()?))?;
    assert_eq!(restored.rows[0].id, original.id);
    assert_eq!(restored.rows[0].created_at, original.created_at);
    assert!(restored.rows[0].stamp > deleted.rows[0].stamp);
    assert!(!restored.rows[0].deleted);
    Ok(())
}
#[test]
fn trust_entity_clock_exhaustion_and_bad_row_identity_do_not_write_or_wrap() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let empty = super::super::load(&c, owner)?;
    let target = TrustTarget::new("fixed.example", 22)?;
    let active = super::super::edit(
        &c,
        owner,
        &empty,
        TrustEdit::Pin(TrustRecord::new(target.clone(), "ssh-ed25519 AAAA".into())?),
    )?;
    let clock = Hlc::new(
        i64::MAX as u64,
        u32::MAX,
        active.clock.ok_or(StoreError::HostTrustRecord)?.node(),
    )?;
    c.execute("UPDATE known_hosts_state SET clock=?1", [clock.encode()])?;
    let full = load(&c, owner)?;
    assert!(matches!(
        super::super::edit(&c, owner, &full, TrustEdit::Forget(target)),
        Err(StoreError::HostTrustRevision)
    ));
    assert_eq!(load(&c, owner)?.revision(), 1);
    c.execute("UPDATE known_hosts SET id=?1", [Uuid::new_v4().to_string()])?;
    assert!(matches!(load(&c, owner), Err(StoreError::HostTrustRecord)));
    Ok(())
}
#[test]
fn trust_entity_tombstone_limit_never_purges_or_commits_extra_delete() -> TestResult {
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let stamp = Hlc::new(1, 0, Uuid::now_v7())?;
    catalog(&c, stamp)?;
    row(&c, "active.example", "ssh-ed25519 AAAA", 0, stamp)?;
    for i in 0..MAX_DELETED {
        c.execute("INSERT INTO known_hosts(id,created_at,hlc,deleted,host,port,key,ordinal) VALUES (?1,1,?2,1,?3,22,NULL,?4)",rusqlite::params![Uuid::now_v7().to_string(),stamp.encode(),format!("d{i}.example"),(i+1)as i64])?;
    }
    let before = load(&c, owner)?;
    assert!(matches!(
        super::super::edit(
            &c,
            owner,
            &before,
            TrustEdit::Forget(TrustTarget::new("active.example", 22)?)
        ),
        Err(StoreError::HostTrustRecord)
    ));
    let after = load(&c, owner)?;
    assert_eq!(after.revision(), before.revision());
    assert!(after.rows == before.rows);
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
fn trust_schema4_migration_preserves_order_revision_backup_and_empty_tombstone() -> TestResult {
    for empty in [false, true] {
        let dir = private()?;
        let path = dir.path().join("state.db");
        let c = database::open_v3_fixture(&path)?;
        let owner = Uuid::new_v4();
        let mut snapshot = super::super::load(&c, owner)?;
        for host in ["z.example", "a.example"] {
            snapshot = super::super::edit(
                &c,
                owner,
                &snapshot,
                TrustEdit::Pin(TrustRecord::new(
                    TrustTarget::new(host, 22)?,
                    "ssh-ed25519 AAAA".into(),
                )?),
            )?;
        }
        if empty {
            for host in ["z.example", "a.example"] {
                snapshot = super::super::edit(
                    &c,
                    owner,
                    &snapshot,
                    TrustEdit::Forget(TrustTarget::new(host, 22)?),
                )?;
            }
        }
        drop(c);
        let c = database::open_v4_trust_fixture(&path)?;
        assert_eq!(
            c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))?,
            4
        );
        let restored = load(&c, owner)?;
        assert_eq!(restored.revision(), snapshot.revision());
        assert_eq!(restored.records().len(), snapshot.records().len());
        for (a, b) in restored.records().iter().zip(snapshot.records()) {
            assert!(a.target() == b.target());
            assert_eq!(a.key(), b.key());
        }
        assert!(!c.query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE key=?1)",
            [KEY],
            |r| r.get::<_, bool>(0)
        )?);
        let backup = Connection::open(path.with_extension("db.bak-4"))?;
        assert_eq!(
            backup.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))?,
            3
        );
        assert_eq!(
            super::super::load(&backup, owner)?.revision(),
            snapshot.revision()
        );
    }
    Ok(())
}
#[test]
fn trust_schema4_bad_legacy_rolls_back_whole_schema_and_retains_backup() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let c = database::open_v3_fixture(&path)?;
    c.execute(
        "INSERT INTO app_state(key,value) VALUES (?1,'not-json')",
        [KEY],
    )?;
    drop(c);
    assert!(database::open_v4_trust_fixture(&path).is_err());
    let c = Connection::open(&path)?;
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))?,
        3
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE name IN ('known_hosts','known_hosts_state')",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        0
    );
    assert_eq!(
        c.query_row("SELECT value FROM app_state WHERE key=?1", [KEY], |r| {
            r.get::<_, String>(0)
        })?,
        "not-json"
    );
    assert!(path.with_extension("db.bak-4").is_file());
    assert!(matches!(
        database::open_v4_trust_fixture(&path),
        Err(StoreError::Backup)
    ));
    Ok(())
}
#[test]
fn trust_schema4_bad_trust_rolls_back_prior_vault_identity_steps_from_v1() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let c = database::open_v1_fixture(&path)?;
    c.execute(
        "INSERT INTO app_state(key,value) VALUES (?1,'not-json')",
        [KEY],
    )?;
    drop(c);
    assert!(database::open(&path).is_err());
    let c = Connection::open(&path)?;
    assert_eq!(
        c.pragma_query_value::<i64, _>(None, "user_version", |r| r.get(0))?,
        1
    );
    assert_eq!(
        c.query_row(
            "SELECT count(*) FROM sqlite_schema WHERE type='table' AND name NOT LIKE 'sqlite_%'",
            [],
            |r| r.get::<_, i64>(0)
        )?,
        1
    );
    assert_eq!(
        c.query_row("SELECT value FROM app_state WHERE key=?1", [KEY], |r| {
            r.get::<_, String>(0)
        })?,
        "not-json"
    );
    assert!(path.with_extension("db.bak-5").is_file());
    Ok(())
}
#[test]
fn trust_schema4_row_aggregate_budget_and_wrong_metadata_singleton_are_rejected() -> TestResult {
    for oversized in [false, true] {
        let dir = private()?;
        let c = database::open(&dir.path().join("state.db"))?;
        let stamp = Hlc::new(1, 0, Uuid::now_v7())?;
        catalog(&c, stamp)?;
        let key = if oversized {
            format!("ssh-ed25519 {}", "A".repeat(8180))
        } else {
            "ssh-ed25519 AAAA".into()
        };
        for i in 0..if oversized { 9 } else { 129 } {
            row(&c, &format!("h{i}.example"), &key, i, stamp)?;
        }
        assert!(matches!(
            load(&c, Uuid::new_v4()),
            Err(StoreError::HostTrustRecord)
        ));
    }
    let dir = private()?;
    let c = database::open(&dir.path().join("state.db"))?;
    c.execute_batch("DROP TABLE known_hosts_state; CREATE TABLE known_hosts_state(singleton INTEGER,schema_version INTEGER,revision INTEGER,node_id TEXT,clock TEXT); INSERT INTO known_hosts_state VALUES(2,1,0,NULL,NULL)")?;
    assert!(matches!(
        load(&c, Uuid::new_v4()),
        Err(StoreError::HostTrustRecord)
    ));
    Ok(())
}
#[test]
fn trust_schema4_dual_source_ordinal_oversize_and_revision_zero_rows_are_rejected() -> TestResult {
    let dir = private()?;
    let c = database::open_v4_trust_fixture(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    assert_eq!(load(&c, owner)?.revision(), 0);
    c.execute(
        "INSERT INTO app_state(key,value) VALUES (?1,'not-json')",
        [KEY],
    )?;
    assert!(matches!(load(&c, owner), Err(StoreError::HostTrustRecord)));
    c.execute("DELETE FROM app_state WHERE key=?1", [KEY])?;
    let stamp = Hlc::new(1, 0, Uuid::now_v7())?;
    row(&c, "fixed.example", "ssh-ed25519 AAAA", 0, stamp)?;
    assert!(matches!(load(&c, owner), Err(StoreError::HostTrustRecord)));
    catalog(&c, stamp)?;
    assert_eq!(load(&c, owner)?.records().len(), 1);
    c.execute("UPDATE known_hosts SET ordinal=1", [])?;
    assert!(matches!(load(&c, owner), Err(StoreError::HostTrustRecord)));
    c.execute(
        "UPDATE known_hosts SET ordinal=0, key=?1",
        ["a".repeat(8193)],
    )?;
    assert!(matches!(load(&c, owner), Err(StoreError::HostTrustRecord)));
    Ok(())
}
