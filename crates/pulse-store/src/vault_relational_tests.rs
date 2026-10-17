use crate::{
    StoreError, database,
    vault_crypto::VaultCipher,
    vault_database::{self, VaultEdit, relational},
    vault_record::CredentialRecord,
    vault_wrap::ValidationWrappedDek,
};
use rusqlite::{Connection, Transaction};
use rusqlite_migration::{HookError, M, Migrations};
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
type Fixture = (Connection, Uuid, Uuid, Uuid, Uuid, VaultCipher);
#[tokio::test]
async fn actual_worker_open_migrates_v1_and_reopens_same_relational_ciphertext() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (c, _, vault, alive, _, key) = setup(&path)?;
    let original = legacy(&c)?;
    drop(c);
    let (store, worker) = crate::StateStore::open(path.clone()).await?;
    let value = store
        .load_vault_ciphertext(std::sync::Arc::new(|| true))
        .await?;
    assert_eq!(value.revision(), 4);
    assert_eq!(
        value.credentials()[0]
            .open(vault, alive, "ssh_password", &key)?
            .expose(),
        b"fixed-fixture-secret"
    );
    let encoded = value.credentials()[0].encode_ciphertext()?;
    drop(store);
    worker.join().await?;
    let c = Connection::open(&path)?;
    assert_eq!(version(&c)?, 5);
    assert!(matches!(
        legacy(&c),
        Err(rusqlite::Error::QueryReturnedNoRows)
    ));
    let backup = Connection::open(d.path().join("state.db.bak-5"))?;
    assert_eq!(legacy(&backup)?, original);
    drop(backup);
    drop(c);
    let (store, worker) = crate::StateStore::open(path).await?;
    let value = store
        .load_vault_ciphertext(std::sync::Arc::new(|| true))
        .await?;
    assert_eq!(value.credentials()[0].encode_ciphertext()?, encoded);
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn schema_two_never_falls_back_to_missing_tables_or_shadow_legacy_document() -> TestResult {
    for missing in [false, true] {
        let d = private()?;
        let path = d.path().join("state.db");
        let (mut c, _, _, _, _, _) = setup(&path)?;
        let original = legacy(&c)?;
        database::migrate(&mut c, &path, &migration(false), 2)?;
        if missing {
            c.execute_batch("DROP TABLE secrets;DROP TABLE vault_deleted_credentials;DROP TABLE vault_envelopes;")?;
        } else {
            c.execute(
                "INSERT INTO app_state(key,value) VALUES(?1,?2)",
                rusqlite::params![vault_database::KEY, original],
            )?;
        }
        drop(c);
        assert!(matches!(
            crate::StateStore::open(path.clone()).await,
            Err(StoreError::Database)
        ));
        let c = Connection::open(path)?;
        assert_eq!(version(&c)?, 2);
        if !missing {
            assert!(legacy(&c).is_ok());
        }
    }
    Ok(())
}
#[test]
fn occupied_schema_destination_does_not_overwrite_legacy_or_destination() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (mut c, _, _, _, _, _) = setup(&path)?;
    let original = legacy(&c)?;
    c.execute_batch(
        "CREATE TABLE vault_envelopes(sentinel TEXT);INSERT INTO vault_envelopes VALUES('keep');",
    )?;
    assert!(matches!(
        database::migrate(&mut c, &path, &migration(false), 2),
        Err(StoreError::Database)
    ));
    assert_eq!(version(&c)?, 1);
    assert_eq!(legacy(&c)?, original);
    let sentinel: String = c.query_row("SELECT sentinel FROM vault_envelopes", [], |r| r.get(0))?;
    assert_eq!(sentinel, "keep");
    Ok(())
}

const V1: &str =
    "CREATE TABLE app_state(key TEXT PRIMARY KEY NOT NULL,value TEXT NOT NULL) STRICT;";
fn migration(fail_after_import: bool) -> Migrations<'static> {
    Migrations::new(vec![
        M::up(V1),
        M::up_with_hook(relational::SCHEMA, move |t: &Transaction<'_>| {
            relational::import_legacy(t)
                .map_err(|_| HookError::Hook("vault.import_denied".into()))?;
            if fail_after_import {
                return Err(HookError::Hook("fixture.rollback".into()));
            }
            Ok(())
        }),
    ])
}
fn private() -> std::io::Result<tempfile::TempDir> {
    let d = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(d)
}
fn setup(path: &std::path::Path) -> Result<Fixture, Box<dyn std::error::Error>> {
    let c = database::open_v1_fixture(path)?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
    let alive = Uuid::now_v7();
    let deleted = Uuid::now_v7();
    let key = VaultCipher::generate()?;
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let envelope = ValidationWrappedDek::decode_ciphertext(&bytes, vault)?;
    let mut s = vault_database::load(&c, owner)?;
    s = vault_database::edit(
        &c,
        owner,
        &s,
        VaultEdit::Initialize(vault, envelope),
        || true,
    )?;
    for id in [alive, deleted] {
        s = vault_database::edit(
            &c,
            owner,
            &s,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                id,
                "ssh_password",
                &key,
                b"fixed-fixture-secret",
            )?),
            || true,
        )?;
    }
    vault_database::edit(&c, owner, &s, VaultEdit::Remove(deleted), || true)?;
    c.execute(
        "INSERT INTO app_state(key,value) VALUES('fixture.keep','untouched')",
        [],
    )?;
    Ok((c, owner, vault, alive, deleted, key))
}
fn legacy(c: &Connection) -> rusqlite::Result<String> {
    c.query_row(
        "SELECT value FROM app_state WHERE key=?1",
        [vault_database::KEY],
        |r| r.get(0),
    )
}
fn version(c: &Connection) -> rusqlite::Result<u32> {
    c.pragma_query_value(None, "user_version", |r| r.get(0))
}
#[test]
fn encrypted_import_preserves_bytes_revision_tombstones_backup_and_cas() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (mut c, owner, vault, alive, deleted, key) = setup(&path)?;
    let original = legacy(&c)?;
    let before = vault_database::load(&c, owner)?;
    database::migrate(&mut c, &path, &migration(false), 2)?;
    assert_eq!(version(&c)?, 2);
    assert!(matches!(
        legacy(&c),
        Err(rusqlite::Error::QueryReturnedNoRows)
    ));
    let after = vault_database::load(&c, owner)?;
    assert_eq!(after.revision(), 4);
    assert_eq!(
        before
            .envelope()
            .ok_or("fixture.envelope")?
            .encode_ciphertext()?,
        after
            .envelope()
            .ok_or("fixture.envelope")?
            .encode_ciphertext()?
    );
    assert_eq!(
        before.credentials()[0].encode_ciphertext()?,
        after.credentials()[0].encode_ciphertext()?
    );
    assert_eq!(
        after.credentials()[0]
            .open(vault, alive, "ssh_password", &key)?
            .expose(),
        b"fixed-fixture-secret"
    );
    let keep: String = c.query_row(
        "SELECT value FROM app_state WHERE key='fixture.keep'",
        [],
        |r| r.get(0),
    )?;
    assert_eq!(keep, "untouched");
    let backup = Connection::open(d.path().join("state.db.bak-2"))?;
    assert_eq!(version(&backup)?, 1);
    assert_eq!(legacy(&backup)?, original);
    assert!(matches!(
        vault_database::edit(
            &c,
            owner,
            &after,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                deleted,
                "ssh_password",
                &key,
                b"new"
            )?),
            || true
        ),
        Err(StoreError::VaultConflict)
    ));
    let next = vault_database::edit(
        &c,
        owner,
        &after,
        VaultEdit::Replace(CredentialRecord::seal(
            vault,
            alive,
            "ssh_password",
            &key,
            b"new",
        )?),
        || true,
    )?;
    assert_eq!(next.revision(), 5);
    assert!(matches!(
        vault_database::edit(&c, owner, &after, VaultEdit::Remove(alive), || true),
        Err(StoreError::VaultConflict)
    ));
    Ok(())
}
#[test]
fn corrupt_source_or_post_import_failure_rolls_back_tables_delete_and_version() -> TestResult {
    for malformed in [false, true] {
        let d = private()?;
        let path = d.path().join("state.db");
        let (mut c, _, _, _, _, _) = setup(&path)?;
        if malformed {
            c.execute(
                "UPDATE app_state SET value='malformed' WHERE key=?1",
                [vault_database::KEY],
            )?;
        }
        let original = legacy(&c)?;
        assert!(matches!(
            database::migrate(&mut c, &path, &migration(!malformed), 2),
            Err(StoreError::Database)
        ));
        assert_eq!(version(&c)?, 1);
        assert_eq!(legacy(&c)?, original);
        assert!(!relational::exists(&c)?);
        let backup = Connection::open(d.path().join("state.db.bak-2"))?;
        assert_eq!(legacy(&backup)?, original);
    }
    Ok(())
}
#[test]
fn occupied_backup_and_destination_do_not_overwrite_either_source() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (mut c, _, _, _, _, _) = setup(&path)?;
    let original = legacy(&c)?;
    let backup = d.path().join("state.db.bak-2");
    std::fs::write(&backup, b"foreign-backup")?;
    assert!(matches!(
        database::migrate(&mut c, &path, &migration(false), 2),
        Err(StoreError::Backup)
    ));
    assert_eq!(std::fs::read(&backup)?, b"foreign-backup");
    assert_eq!(legacy(&c)?, original);
    assert_eq!(version(&c)?, 1);
    Ok(())
}
#[test]
fn relational_read_stays_at_one_wal_snapshot_while_other_connection_updates() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (mut c, owner, vault, alive, _, key) = setup(&path)?;
    database::migrate(&mut c, &path, &migration(false), 2)?;
    let second = Connection::open(&path)?;
    let read = c.transaction()?;
    let old = vault_database::load(&read, owner)?;
    let candidate = vault_database::load(&second, owner)?;
    vault_database::edit(
        &second,
        owner,
        &candidate,
        VaultEdit::Replace(CredentialRecord::seal(
            vault,
            alive,
            "ssh_password",
            &key,
            b"advanced",
        )?),
        || true,
    )?;
    let pinned = vault_database::load(&read, owner)?;
    assert_eq!(pinned.revision(), old.revision());
    assert_eq!(
        pinned.credentials()[0]
            .open(vault, alive, "ssh_password", &key)?
            .expose(),
        b"fixed-fixture-secret"
    );
    read.rollback()?;
    let fresh = vault_database::load(&c, owner)?;
    assert_eq!(fresh.revision(), 5);
    assert_eq!(
        fresh.credentials()[0]
            .open(vault, alive, "ssh_password", &key)?
            .expose(),
        b"advanced"
    );
    Ok(())
}
