use crate::{
    StateStore, StoreError, database,
    ssh_identities::{self, IdentityEdit},
    vault_database::{self, VaultEdit},
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
};
use std::sync::Arc;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[test]
fn same_revision_changes_and_exhausted_clocks_reject_without_overwrite() -> TestResult {
    for exhausted in [false, true] {
        let d = private()?;
        let owner = Uuid::new_v4();
        let (c, vault) = setup(&d.path().join("state.db"), owner)?;
        let empty = ssh_identities::load(&c, owner)?;
        let saved = ssh_identities::edit_at(
            &c,
            owner,
            &empty,
            IdentityEdit::Create(value(vault, Uuid::now_v7(), "original", false)?),
            100,
            || true,
        )?;
        let id = saved.records().next().ok_or("fixture.row")?.id();
        if exhausted {
            c.execute("UPDATE identity_catalog SET revision=?1", [i64::MAX])?;
            let current = ssh_identities::load(&c, owner)?;
            assert!(matches!(
                ssh_identities::edit_at(&c, owner, &current, IdentityEdit::Remove(id), 101, || {
                    true
                }),
                Err(StoreError::IdentityExhausted)
            ));
        } else {
            c.execute("UPDATE identities SET label='changed'", [])?;
            assert!(matches!(
                ssh_identities::edit_at(&c, owner, &saved, IdentityEdit::Remove(id), 101, || true),
                Err(StoreError::IdentityConflict)
            ));
        }
        assert_eq!(ssh_identities::load(&c, owner)?.records().count(), 1);
    }
    Ok(())
}
#[test]
fn all_identity_active_deleted_and_hlc_budgets_fail_closed() -> TestResult {
    for deleted in [false, true] {
        let d = private()?;
        let owner = Uuid::new_v4();
        let (c, vault) = setup(&d.path().join("state.db"), owner)?;
        let empty = ssh_identities::load(&c, owner)?;
        let secret = Uuid::now_v7();
        let saved = ssh_identities::edit_at(
            &c,
            owner,
            &empty,
            IdentityEdit::Create(value(vault, secret, "one", false)?),
            100,
            || true,
        )?;
        let first = saved.records().next().ok_or("fixture.row")?;
        let stamp = first.stamp().encode();
        let count = if deleted { 4096 } else { 255 };
        {
            let mut statement=c.prepare("INSERT INTO identities(id,created_at,hlc,deleted,label,username,vault_id,secret_id,kind,format) VALUES(?1,100,?2,?3,'fixture','fixed-user',?4,?5,'ssh_password',1)")?;
            for _ in 0..count {
                statement.execute(rusqlite::params![
                    Uuid::now_v7().to_string(),
                    stamp,
                    i64::from(deleted),
                    vault.to_string(),
                    secret.to_string()
                ])?;
            }
        }
        let current = ssh_identities::load(&c, owner)?;
        let intent = if deleted {
            IdentityEdit::Remove(first.id())
        } else {
            IdentityEdit::Create(value(vault, secret, "overflow", false)?)
        };
        assert!(matches!(
            ssh_identities::edit_at(&c, owner, &current, intent, 101, || true),
            Err(StoreError::IdentityExhausted)
        ));
        let node = first.stamp().node();
        let maximum = pulse_core::hlc::Hlc::new(i64::MAX as u64, u32::MAX, node)?.encode();
        c.execute("UPDATE identity_catalog SET clock=?1", [maximum])?;
        let current = ssh_identities::load(&c, owner)?;
        assert!(matches!(
            ssh_identities::edit_at(
                &c,
                owner,
                &current,
                IdentityEdit::Update(first.id(), value(vault, secret, "clock-overflow", false)?),
                100,
                || true
            ),
            Err(StoreError::IdentityExhausted)
        ));
    }
    Ok(())
}
#[tokio::test]
async fn referenced_secret_deletion_changes_presence_not_identity_revision_or_authority()
-> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (c, vault) = setup(&path, Uuid::new_v4())?;
    drop(c);
    let (store, worker) = StateStore::open(path).await?;
    let old = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    let secret = Uuid::now_v7();
    let key = crate::vault_crypto::VaultCipher::generate()?;
    let existing = store
        .edit_vault_ciphertext(
            old,
            VaultEdit::Create(crate::vault_record::CredentialRecord::seal(
                vault,
                secret,
                "ssh_password",
                &key,
                b"fixture",
            )?),
            Arc::new(|| true),
        )
        .await?;
    let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
    let saved = store
        .edit_ssh_identities(
            empty,
            IdentityEdit::Create(value(vault, secret, "one", false)?),
            Arc::new(|| true),
        )
        .await?;
    let id = saved.records().next().ok_or("fixture.row")?.id();
    assert!(
        saved
            .records()
            .next()
            .ok_or("fixture.row")?
            .identity()
            .password()
            .has_value()
    );
    store
        .edit_vault_ciphertext(existing, VaultEdit::Remove(secret), Arc::new(|| true))
        .await?;
    let current = store.load_ssh_identities(Arc::new(|| true)).await?;
    assert_eq!(current.revision(), saved.revision());
    assert!(
        !current
            .records()
            .next()
            .ok_or("fixture.row")?
            .identity()
            .password()
            .has_value()
    );
    let renamed = store
        .edit_ssh_identities(
            saved,
            IdentityEdit::Update(id, value(vault, secret, "renamed", true)?),
            Arc::new(|| true),
        )
        .await?;
    assert!(
        !renamed
            .records()
            .next()
            .ok_or("fixture.row")?
            .identity()
            .password()
            .has_value()
    );
    assert!(matches!(
        store
            .edit_ssh_identities(
                renamed,
                IdentityEdit::Create(value(vault, secret, "cannot-new-link", true)?),
                Arc::new(|| true)
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    drop(store);
    worker.join().await?;
    Ok(())
}
#[test]
fn separate_identity_processes_preserve_node_stamp_and_tombstone() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let owner = Uuid::new_v4();
    let (c, vault) = setup(&path, owner)?;
    let empty = ssh_identities::load(&c, owner)?;
    let saved = ssh_identities::edit_at(
        &c,
        owner,
        &empty,
        IdentityEdit::Create(value(vault, Uuid::now_v7(), "seed", false)?),
        100,
        || true,
    )?;
    let row = saved.records().next().ok_or("fixture.row")?;
    let id = row.id();
    let stamp = row.stamp().encode();
    drop(c);
    for mode in ["update", "remove", "reopen"] {
        let output = std::process::Command::new(std::env::current_exe()?)
            .args([
                "--exact",
                "ssh_identity_tests::identity_process_worker",
                "--ignored",
                "--nocapture",
            ])
            .env("PULSE_IDENTITY_FIXTURE_DB", &path)
            .env("PULSE_IDENTITY_FIXTURE_MODE", mode)
            .env("PULSE_IDENTITY_FIXTURE_ID", id.to_string())
            .env("PULSE_IDENTITY_FIXTURE_STAMP", &stamp)
            .output()?;
        assert!(output.status.success(), "fixture.identity.child_failed");
        let expected = format!("fixture.identity.{mode}.confirmed");
        assert!(
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .any(|line| line == expected)
        );
    }
    let c = database::open(&path)?;
    let read = ssh_identities::load(&c, Uuid::new_v4())?;
    assert_eq!(read.revision(), 3);
    assert_eq!(read.records().count(), 0);
    Ok(())
}
#[test]
#[ignore = "fixed child, explicitly exercised by parent; never zero-test acceptance"]
fn identity_process_worker() -> TestResult {
    let path = std::path::PathBuf::from(std::env::var("PULSE_IDENTITY_FIXTURE_DB")?);
    let mode = std::env::var("PULSE_IDENTITY_FIXTURE_MODE")?;
    let id = Uuid::parse_str(&std::env::var("PULSE_IDENTITY_FIXTURE_ID")?)?;
    let original = pulse_core::hlc::Hlc::parse(&std::env::var("PULSE_IDENTITY_FIXTURE_STAMP")?)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let (store, worker) = StateStore::open(path).await?;
        let snapshot = store.load_ssh_identities(Arc::new(|| true)).await?;
        match mode.as_str() {
            "update" => {
                let row = snapshot.records().next().ok_or("fixture.row")?;
                assert_eq!(row.id(), id);
                assert!(row.stamp() == original);
                let vault = row.identity().password().vault_id();
                let secret = row.identity().password().id();
                let saved = store
                    .edit_ssh_identities(
                        snapshot,
                        IdentityEdit::Update(id, value(vault, secret, "updated", true)?),
                        Arc::new(|| true),
                    )
                    .await?;
                let row = saved.records().next().ok_or("fixture.row")?;
                assert!(row.stamp() > original);
                assert_eq!(row.stamp().node(), original.node());
            }
            "remove" => {
                let row = snapshot.records().next().ok_or("fixture.row")?;
                assert_eq!(row.identity().label(), "updated");
                assert_eq!(row.stamp().node(), original.node());
                store
                    .edit_ssh_identities(snapshot, IdentityEdit::Remove(id), Arc::new(|| true))
                    .await?;
            }
            "reopen" => {
                assert_eq!(snapshot.records().count(), 0);
                assert_eq!(snapshot.revision(), 3);
                assert!(matches!(
                    store
                        .edit_ssh_identities(snapshot, IdentityEdit::Remove(id), Arc::new(|| true))
                        .await,
                    Err(StoreError::IdentityConflict)
                ));
            }
            _ => return Err("fixture.mode".into()),
        };
        drop(store);
        worker.join().await?;
        println!("fixture.identity.{mode}.confirmed");
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}

#[tokio::test]
async fn actual_schema_two_upgrade_preserves_vault_and_backup_then_creates_identity() -> TestResult
{
    let d = private()?;
    let path = d.path().join("state.db");
    let c = database::open_v2_fixture(&path)?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(vault.as_bytes());
    let absent = vault_database::load(&c, owner)?;
    let old = vault_database::edit(
        &c,
        owner,
        &absent,
        VaultEdit::Initialize(
            vault,
            ValidationWrappedDek::decode_ciphertext(&bytes, vault)?,
        ),
        || true,
    )?;
    let encoded = old
        .envelope()
        .ok_or("fixture.envelope")?
        .encode_ciphertext()?;
    drop(c);
    let (store, worker) = StateStore::open(path.clone()).await?;
    let read = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    assert_eq!(
        read.envelope()
            .ok_or("fixture.envelope")?
            .encode_ciphertext()?,
        encoded
    );
    let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
    assert_eq!(empty.revision(), 0);
    store
        .edit_ssh_identities(
            empty,
            IdentityEdit::Create(value(vault, Uuid::now_v7(), "after-upgrade", true)?),
            Arc::new(|| true),
        )
        .await?;
    drop(store);
    worker.join().await?;
    let c = rusqlite::Connection::open(path)?;
    assert_eq!(
        c.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        5
    );
    let backup = rusqlite::Connection::open(d.path().join("state.db.bak-5"))?;
    assert_eq!(
        backup.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
        2
    );
    assert_eq!(
        vault_database::load(&backup, owner)?
            .envelope()
            .ok_or("fixture.envelope")?
            .encode_ciphertext()?,
        encoded
    );
    Ok(())
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
fn setup(
    path: &std::path::Path,
    owner: Uuid,
) -> Result<(rusqlite::Connection, Uuid), Box<dyn std::error::Error>> {
    let c = database::open(path)?;

    let id = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    let s = vault_database::load(&c, owner)?;
    vault_database::edit(
        &c,
        owner,
        &s,
        VaultEdit::Initialize(id, ValidationWrappedDek::decode_ciphertext(&bytes, id)?),
        || true,
    )?;
    Ok((c, id))
}
fn value(
    vault: Uuid,
    secret: Uuid,
    label: &str,
    has: bool,
) -> Result<SshPasswordIdentity, Box<dyn std::error::Error>> {
    Ok(SshPasswordIdentity::new(
        label,
        "fixed-user",
        SecretRef::new(vault, secret, SecretKind::SshPassword, has)?,
    )?)
}
#[test]
fn identity_metadata_reopen_hlc_skew_tombstone_and_stale_foreign_capture() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let owner = Uuid::new_v4();
    let (c, vault) = setup(&path, owner)?;
    let empty = ssh_identities::load(&c, owner)?;
    let secret = Uuid::now_v7();
    let first = ssh_identities::edit_at(
        &c,
        owner,
        &empty,
        IdentityEdit::Create(value(vault, secret, "one", true)?),
        100,
        || true,
    )?;
    let row = first.records().next().ok_or("fixture.row")?;
    assert!(!row.identity().password().has_value());
    let id = row.id();
    let created = row.created_at();
    let first_stamp = row.stamp();
    let changed = ssh_identities::edit_at(
        &c,
        owner,
        &first,
        IdentityEdit::Update(id, value(vault, secret, "two", false)?),
        99,
        || true,
    )?;
    let row = changed.records().next().ok_or("fixture.row")?;
    assert_eq!(row.created_at(), created);
    assert!(row.stamp() > first_stamp);
    assert_eq!(row.stamp().node(), first_stamp.node());
    assert!(matches!(
        ssh_identities::edit_at(&c, owner, &first, IdentityEdit::Remove(id), 101, || true),
        Err(StoreError::IdentityConflict)
    ));
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            Uuid::new_v4(),
            &changed,
            IdentityEdit::Remove(id),
            101,
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    let removed =
        ssh_identities::edit_at(&c, owner, &changed, IdentityEdit::Remove(id), 100, || true)?;
    assert_eq!(removed.records().count(), 0);
    assert_eq!(removed.revision(), 3);
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            owner,
            &removed,
            IdentityEdit::Update(id, value(vault, secret, "resurrect", true)?),
            102,
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    drop(c);
    let c = database::open(&path)?;
    let reopened = ssh_identities::load(&c, Uuid::new_v4())?;
    assert_eq!(reopened.revision(), 3);
    assert_eq!(reopened.records().count(), 0);
    Ok(())
}
#[test]
fn invalid_ref_source_or_precommit_revoke_cannot_create_metadata() -> TestResult {
    let d = private()?;
    let owner = Uuid::new_v4();
    let (c, vault) = setup(&d.path().join("state.db"), owner)?;
    let empty = ssh_identities::load(&c, owner)?;
    let secret = Uuid::now_v7();
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            owner,
            &empty,
            IdentityEdit::Create(value(Uuid::now_v7(), secret, "foreign", true)?),
            100,
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    let calls = std::sync::atomic::AtomicUsize::new(0);
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            owner,
            &empty,
            IdentityEdit::Create(value(vault, secret, "revoked", true)?),
            100,
            || calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0
        ),
        Err(StoreError::IdentityAuthorization)
    ));
    assert_eq!(ssh_identities::load(&c, owner)?.revision(), 0);
    let valid = ssh_identities::edit_at(
        &c,
        owner,
        &empty,
        IdentityEdit::Create(value(vault, secret, "one", false)?),
        100,
        || true,
    )?;
    c.execute_batch(
        "PRAGMA ignore_check_constraints=ON;UPDATE identities SET kind='ssh_key_passphrase';",
    )?;
    assert!(matches!(
        ssh_identities::load(&c, owner),
        Err(StoreError::IdentityRecord)
    ));
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            owner,
            &valid,
            IdentityEdit::Remove(valid.records().next().ok_or("fixture.row")?.id()),
            101,
            || true
        ),
        Err(StoreError::IdentityRecord)
    ));
    Ok(())
}
#[tokio::test]
async fn actual_worker_derives_unfilled_reference_and_retains_catalog_after_reopen() -> TestResult {
    let d = private()?;
    let path = d.path().join("state.db");
    let (c, vault) = setup(&path, Uuid::new_v4())?;
    drop(c);
    let (store, worker) = StateStore::open(path.clone()).await?;
    let empty = store.load_ssh_identities(Arc::new(|| true)).await?;
    let saved = store
        .edit_ssh_identities(
            empty,
            IdentityEdit::Create(value(vault, Uuid::now_v7(), "native", true)?),
            Arc::new(|| true),
        )
        .await?;
    let row = saved.records().next().ok_or("fixture.row")?;
    let id = row.id();
    let stamp = row.stamp();
    assert!(!row.identity().password().has_value());
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path).await?;
    let read = store.load_ssh_identities(Arc::new(|| true)).await?;
    let row = read.records().next().ok_or("fixture.row")?;
    assert_eq!(row.id(), id);
    assert!(row.stamp() == stamp);
    assert_eq!(read.revision(), 1);
    drop(store);
    worker.join().await?;
    Ok(())
}

#[test]
fn batch_removal_preserves_tombstones_and_rolls_back_bad_or_revoked_selection() -> TestResult {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = private()?;
    let owner = Uuid::new_v4();
    let path = dir.path().join("state.db");
    let (c, vault) = setup(&path, owner)?;
    let mut snapshot = ssh_identities::load(&c, owner)?;
    for name in ["first", "second", "survivor"] {
        snapshot = ssh_identities::edit_at(
            &c,
            owner,
            &snapshot,
            IdentityEdit::Create(value(vault, Uuid::now_v7(), name, false)?),
            100,
            || true,
        )?;
    }
    let ids: Vec<_> = snapshot.records().take(2).map(|r| r.id()).collect();
    for invalid in [
        vec![],
        vec![ids[0], ids[0]],
        vec![ids[0], Uuid::now_v7()],
        vec![ids[0]; 257],
    ] {
        assert!(
            ssh_identities::edit_at(
                &c,
                owner,
                &snapshot,
                IdentityEdit::RemoveMany(invalid),
                101,
                || true
            )
            .is_err()
        );
        assert_eq!(ssh_identities::load(&c, owner)?.revision(), 3);
    }
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        ssh_identities::edit_at(
            &c,
            owner,
            &snapshot,
            IdentityEdit::RemoveMany(ids.clone()),
            101,
            || calls.fetch_add(1, Ordering::SeqCst) == 0
        ),
        Err(StoreError::IdentityAuthorization)
    ));
    assert_eq!(ssh_identities::load(&c, owner)?.records().count(), 3);
    let saved = ssh_identities::edit_at(
        &c,
        owner,
        &snapshot,
        IdentityEdit::RemoveMany(ids.clone()),
        101,
        || true,
    )?;
    assert_eq!(saved.revision(), 4);
    assert_eq!(saved.records().count(), 1);
    let (deleted, same_stamp): (i64, bool) = c.query_row(
        "SELECT count(*),min(hlc)=max(hlc) FROM identities WHERE deleted=1",
        [],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(deleted, 2);
    assert!(same_stamp);
    assert!(
        ssh_identities::edit_at(
            &c,
            owner,
            &snapshot,
            IdentityEdit::RemoveMany(ids),
            102,
            || true
        )
        .is_err()
    );
    drop(c);
    let c = database::open(&path)?;
    assert_eq!(ssh_identities::load(&c, owner)?.records().count(), 1);
    Ok(())
}
