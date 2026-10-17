use crate::{
    StateStore, StoreError, ssh_key_identities::KeyIdentityEdit, vault_database::VaultEdit,
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_key_identity::SshPrivateKeyIdentity,
};

#[tokio::test]
async fn joint_source_rejects_new_vault_changed_directory_ciphertext_and_foreign_instance()
-> TestResult {
    use crate::{vault_crypto::VaultCipher, vault_record::CredentialRecord};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (a, wa) = StateStore::open(path.clone()).await?;
    let before_vault = a.load_key_identity_vault_source(allowed()).await?;
    let vault = initialize(&a).await?;
    assert!(matches!(
        a.validate_key_identity_vault_source(before_vault, allowed())
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let before_identity = a.load_key_identity_vault_source(allowed()).await?;
    let key = Uuid::now_v7();
    a.edit_ssh_key_identities(
        before_identity.identities().clone(),
        KeyIdentityEdit::Create(identity(vault, "joint", key, None)?),
        allowed(),
    )
    .await?;
    assert!(matches!(
        a.validate_key_identity_vault_source(before_identity, allowed())
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let before_ciphertext = a.load_key_identity_vault_source(allowed()).await?;
    a.validate_key_identity_vault_source(before_ciphertext.clone(), allowed())
        .await?;
    let cipher = VaultCipher::generate()?;
    a.edit_vault_ciphertext(
        before_ciphertext.vault().clone(),
        VaultEdit::Create(CredentialRecord::seal(
            vault,
            key,
            "ssh_private_key",
            &cipher,
            b"fixed-joint-key",
        )?),
        allowed(),
    )
    .await?;
    assert!(matches!(
        a.validate_key_identity_vault_source(before_ciphertext, allowed())
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let current = a.load_key_identity_vault_source(allowed()).await?;
    assert!(
        current
            .identities()
            .records()
            .next()
            .ok_or("fixture.current")?
            .identity()
            .private_key()
            .has_value()
    );
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    assert!(matches!(
        a.validate_key_identity_vault_source(
            current.clone(),
            Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) == 0)
        )
        .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(matches!(
        a.load_key_identity_vault_source(Arc::new(|| panic!("fixed-authorizer-panic")))
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let (b, wb) = StateStore::open(path).await?;
    assert!(matches!(
        b.validate_key_identity_vault_source(current.clone(), allowed())
            .await,
        Err(StoreError::IdentityConflict)
    ));
    b.validate_key_identity_vault_source(
        b.load_key_identity_vault_source(allowed()).await?,
        allowed(),
    )
    .await?;
    a.validate_key_identity_vault_source(current, allowed())
        .await?;
    drop(a);
    drop(b);
    wa.join().await?;
    wb.join().await?;
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_directory_read_joint_read_and_validation_retain_credit_until_native_exit()
-> TestResult {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Credit(Arc<AtomicUsize>);
    impl Drop for Credit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    initialize(&store).await?;
    for mode in 0..3 {
        let source = store.load_key_identity_vault_source(allowed()).await?;
        let outstanding = Arc::new(AtomicUsize::new(1));
        let credit = Credit(outstanding.clone());
        let first = AtomicUsize::new(0);
        let (entered, received) = tokio::sync::oneshot::channel();
        let entered = Mutex::new(Some(entered));
        let (release, blocked) = std::sync::mpsc::channel();
        let blocked = Mutex::new(blocked);
        let authorize = Arc::new(move || {
            let _retained = &credit;
            if first.fetch_add(1, Ordering::SeqCst) == 0 {
                if let Some(sender) = entered.lock().ok().and_then(|mut entry| entry.take()) {
                    let _ = sender.send(());
                }
                return blocked.lock().ok().is_some_and(|receiver| {
                    receiver
                        .recv_timeout(std::time::Duration::from_secs(3))
                        .is_ok()
                });
            }
            true
        });
        let client = store.clone();
        let task = tokio::spawn(async move {
            match mode {
                0 => client.load_ssh_key_identities(authorize).await.map(|_| ()),
                1 => client
                    .load_key_identity_vault_source(authorize)
                    .await
                    .map(|_| ()),
                _ => {
                    client
                        .validate_key_identity_vault_source(source, authorize)
                        .await
                }
            }
        });
        tokio::time::timeout(std::time::Duration::from_secs(2), received).await??;
        task.abort();
        let _ = task.await;
        assert_eq!(outstanding.load(Ordering::SeqCst), 1);
        release.send(())?;
        let source = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            store.load_key_identity_vault_source(allowed()),
        )
        .await??;
        assert_eq!(outstanding.load(Ordering::SeqCst), 0);
        assert_eq!(source.identities().records().count(), 0);
    }
    drop(store);
    worker.join().await?;
    Ok(())
}

#[tokio::test]
async fn real_key_and_passphrase_ciphertexts_derive_presence_and_tombstones_do_not_resurrect()
-> TestResult {
    use crate::{vault_crypto::VaultCipher, vault_record::CredentialRecord};
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = initialize(&store).await?;
    let key = Uuid::now_v7();
    let pass = Uuid::now_v7();
    let cipher = VaultCipher::generate()?;
    let source = store.load_vault_ciphertext(allowed()).await?;
    let source = store
        .edit_vault_ciphertext(
            source,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                key,
                "ssh_private_key",
                &cipher,
                b"fixed-key-fixture",
            )?),
            allowed(),
        )
        .await?;
    let source = store
        .edit_vault_ciphertext(
            source,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                pass,
                "ssh_key_passphrase",
                &cipher,
                b"fixed-passphrase-fixture",
            )?),
            allowed(),
        )
        .await?;
    let value = SshPrivateKeyIdentity::new(
        "derived",
        "user",
        SecretRef::new(vault, key, SecretKind::SshPrivateKey, false)?,
        Some(SecretRef::new(
            vault,
            pass,
            SecretKind::SshKeyPassphrase,
            false,
        )?),
    )?;
    let empty = store.load_ssh_key_identities(allowed()).await?;
    let saved = store
        .edit_ssh_key_identities(empty, KeyIdentityEdit::Create(value), allowed())
        .await?;
    let id = saved.records().next().ok_or("fixture.saved")?.id();
    assert!(
        saved
            .records()
            .next()
            .ok_or("fixture.saved")?
            .identity()
            .private_key()
            .has_value()
    );
    assert!(
        saved
            .records()
            .next()
            .ok_or("fixture.saved")?
            .identity()
            .passphrase()
            .ok_or("fixture.passphrase")?
            .has_value()
    );
    store
        .edit_vault_ciphertext(source, VaultEdit::Remove(pass), allowed())
        .await?;
    let changed = store.load_ssh_key_identities(allowed()).await?;
    let row = changed.records().next().ok_or("fixture.changed")?;
    assert!(row.identity().private_key().has_value());
    assert!(
        !row.identity()
            .passphrase()
            .ok_or("fixture.passphrase")?
            .has_value()
    );
    let saved = store
        .edit_ssh_key_identities(
            changed,
            KeyIdentityEdit::Update(id, identity(vault, "label-only", key, Some(pass))?),
            allowed(),
        )
        .await?;
    assert!(
        !saved
            .records()
            .next()
            .ok_or("fixture.saved")?
            .identity()
            .passphrase()
            .ok_or("fixture.passphrase")?
            .has_value()
    );
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path).await?;
    let reopened = store.load_ssh_key_identities(allowed()).await?;
    let row = reopened.records().next().ok_or("fixture.reopened")?;
    assert!(row.identity().private_key().has_value());
    assert!(
        !row.identity()
            .passphrase()
            .ok_or("fixture.passphrase")?
            .has_value()
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
use std::sync::Arc;
use uuid::Uuid;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn private() -> std::io::Result<tempfile::TempDir> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    Ok(dir)
}
fn allowed() -> Arc<dyn Fn() -> bool + Send + Sync> {
    Arc::new(|| true)
}
async fn initialize(store: &StateStore) -> Result<Uuid, Box<dyn std::error::Error>> {
    let id = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    let old = store.load_vault_ciphertext(allowed()).await?;
    store
        .edit_vault_ciphertext(
            old,
            VaultEdit::Initialize(id, ValidationWrappedDek::decode_ciphertext(&bytes, id)?),
            allowed(),
        )
        .await?;
    Ok(id)
}
fn identity(
    vault: Uuid,
    label: &str,
    key: Uuid,
    pass: Option<Uuid>,
) -> Result<SshPrivateKeyIdentity, Box<dyn std::error::Error>> {
    Ok(SshPrivateKeyIdentity::new(
        label,
        "user",
        SecretRef::new(vault, key, SecretKind::SshPrivateKey, true)?,
        pass.map(|id| SecretRef::new(vault, id, SecretKind::SshKeyPassphrase, true))
            .transpose()?,
    )?)
}
#[tokio::test]
async fn rows_derive_two_presences_and_preserve_stamps_across_reopen_and_tombstones() -> TestResult
{
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = initialize(&store).await?;
    let key = Uuid::now_v7();
    let pass = Uuid::now_v7();
    let empty = store.load_ssh_key_identities(allowed()).await?;
    let saved = store
        .edit_ssh_key_identities(
            empty,
            KeyIdentityEdit::Create(identity(vault, "first", key, Some(pass))?),
            allowed(),
        )
        .await?;
    let row = saved.records().next().ok_or("fixture.saved")?;
    let id = row.id();
    let created = row.created_at();
    let stamp = row.stamp();
    assert_eq!(id.get_version_num(), 7);
    assert!(!row.identity().private_key().has_value());
    assert!(
        !row.identity()
            .passphrase()
            .ok_or("fixture.passphrase")?
            .has_value()
    );
    assert_eq!(
        store
            .load_ssh_identities(allowed())
            .await?
            .records()
            .count(),
        0
    );
    let updated = store
        .edit_ssh_key_identities(
            saved.clone(),
            KeyIdentityEdit::Update(id, identity(vault, "edited", key, None)?),
            allowed(),
        )
        .await?;
    let row = updated.records().next().ok_or("fixture.updated")?;
    assert_eq!(row.created_at(), created);
    assert!(row.stamp() > stamp);
    assert!(row.identity().passphrase().is_none());
    assert!(matches!(
        store
            .edit_ssh_key_identities(saved, KeyIdentityEdit::Remove(id), allowed())
            .await,
        Err(StoreError::IdentityConflict)
    ));
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path.clone()).await?;
    let reopened = store.load_ssh_key_identities(allowed()).await?;
    let row = reopened.records().next().ok_or("fixture.reopened")?;
    assert_eq!(row.id(), id);
    assert_eq!(row.created_at(), created);
    assert_eq!(row.identity().label(), "edited");
    let deleted = store
        .edit_ssh_key_identities(reopened, KeyIdentityEdit::Remove(id), allowed())
        .await?;
    assert_eq!(deleted.records().count(), 0);
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                deleted,
                KeyIdentityEdit::Update(id, identity(vault, "cannot-resurrect", key, None)?),
                allowed()
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    drop(store);
    worker.join().await?;
    let c = rusqlite::Connection::open(path)?;
    let tombstone: (i64, i64) = c.query_row(
        "SELECT deleted,created_at FROM key_identities WHERE id=?1",
        [id.to_string()],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    assert_eq!(tombstone, (1, created as i64));
    Ok(())
}
#[tokio::test]
async fn concurrent_instances_reject_foreign_snapshots_and_one_stale_write() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (a, wa) = StateStore::open(path.clone()).await?;
    let vault = initialize(&a).await?;
    let (b, wb) = StateStore::open(path).await?;
    let sa = a.load_ssh_key_identities(allowed()).await?;
    let sb = b.load_ssh_key_identities(allowed()).await?;
    assert!(matches!(
        a.edit_ssh_key_identities(
            sb.clone(),
            KeyIdentityEdit::Create(identity(vault, "foreign", Uuid::now_v7(), None)?),
            allowed()
        )
        .await,
        Err(StoreError::IdentityConflict)
    ));
    let (ra, rb) = tokio::join!(
        a.edit_ssh_key_identities(
            sa,
            KeyIdentityEdit::Create(identity(vault, "a", Uuid::now_v7(), None)?),
            allowed()
        ),
        b.edit_ssh_key_identities(
            sb,
            KeyIdentityEdit::Create(identity(vault, "b", Uuid::now_v7(), None)?),
            allowed()
        )
    );
    assert!(matches!(
        (&ra, &rb),
        (Ok(_), Err(StoreError::IdentityConflict)) | (Err(StoreError::IdentityConflict), Ok(_))
    ));
    assert_eq!(
        a.load_ssh_key_identities(allowed())
            .await?
            .records()
            .count(),
        1
    );
    drop(a);
    drop(b);
    wa.join().await?;
    wb.join().await?;
    Ok(())
}
#[tokio::test]
async fn revoked_predicates_and_missing_or_wrong_role_sources_cannot_save() -> TestResult {
    use crate::{vault_crypto::VaultCipher, vault_record::CredentialRecord};
    use std::sync::atomic::{AtomicUsize, Ordering};
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let empty = store.load_ssh_key_identities(allowed()).await?;
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                empty,
                KeyIdentityEdit::Create(identity(
                    Uuid::now_v7(),
                    "no-vault",
                    Uuid::now_v7(),
                    None
                )?),
                allowed()
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let vault = initialize(&store).await?;
    let key = Uuid::now_v7();
    let pass = Uuid::now_v7();
    let empty = store.load_ssh_key_identities(allowed()).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let seen = calls.clone();
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                empty.clone(),
                KeyIdentityEdit::Create(identity(vault, "revoked", key, None)?),
                Arc::new(move || seen.fetch_add(1, Ordering::SeqCst) == 0)
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert_eq!(
        store.load_ssh_key_identities(allowed()).await?.revision(),
        0
    );
    let cipher = VaultCipher::generate()?;
    let source = store.load_vault_ciphertext(allowed()).await?;
    let source = store
        .edit_vault_ciphertext(
            source,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                key,
                "ssh_password",
                &cipher,
                b"fixed-wrong-role",
            )?),
            allowed(),
        )
        .await?;
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                empty.clone(),
                KeyIdentityEdit::Create(identity(vault, "wrong-role", key, None)?),
                allowed()
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let source = store
        .edit_vault_ciphertext(source, VaultEdit::Remove(key), allowed())
        .await?;
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                empty.clone(),
                KeyIdentityEdit::Create(identity(vault, "deleted-key", key, None)?),
                allowed()
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    let source = store
        .edit_vault_ciphertext(
            source,
            VaultEdit::Create(CredentialRecord::seal(
                vault,
                pass,
                "ssh_key_passphrase",
                &cipher,
                b"fixed-passphrase",
            )?),
            allowed(),
        )
        .await?;
    store
        .edit_vault_ciphertext(source, VaultEdit::Remove(pass), allowed())
        .await?;
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                empty,
                KeyIdentityEdit::Create(identity(
                    vault,
                    "deleted-passphrase",
                    Uuid::now_v7(),
                    Some(pass)
                )?),
                allowed()
            )
            .await,
        Err(StoreError::IdentityConflict)
    ));
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn aliasing_key_and_passphrase_in_sql_is_rejected_on_load_and_reopen() -> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = initialize(&store).await?;
    let empty = store.load_ssh_key_identities(allowed()).await?;
    store
        .edit_ssh_key_identities(
            empty,
            KeyIdentityEdit::Create(identity(vault, "key", Uuid::now_v7(), None)?),
            allowed(),
        )
        .await?;
    let c = rusqlite::Connection::open(&path)?;
    c.execute("UPDATE key_identities SET passphrase_id=secret_id", [])?;
    c.close().map_err(|(_, e)| e)?;
    assert!(matches!(
        store.load_ssh_key_identities(allowed()).await,
        Err(StoreError::IdentityRecord)
    ));
    drop(store);
    worker.join().await?;
    assert!(matches!(
        StateStore::open(path).await,
        Err(StoreError::IdentityRecord)
    ));
    Ok(())
}
#[test]
fn v4_upgrade_retains_backup_and_corrupt_old_identity_rolls_back() -> TestResult {
    let dir = private()?;
    for corrupt in [false, true] {
        let path = dir
            .path()
            .join(if corrupt { "corrupt.db" } else { "valid.db" });
        let c = crate::database::open_v4_trust_fixture(&path)?;
        if corrupt {
            let clock = pulse_core::hlc::Hlc::new(1, 0, Uuid::now_v7())?;
            c.execute(
                "INSERT INTO identity_catalog VALUES(1,1,1,?1,?2)",
                rusqlite::params![Uuid::nil().to_string(), clock.encode()],
            )?;
        }
        c.close().map_err(|(_, e)| e)?;
        let upgraded = crate::database::open(&path);
        assert_eq!(upgraded.is_err(), corrupt);
        if let Ok(c) = upgraded {
            assert_eq!(
                c.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
                5
            );
            c.close().map_err(|(_, e)| e)?;
        }
        let c = rusqlite::Connection::open(&path)?;
        assert_eq!(
            c.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            if corrupt { 4 } else { 5 }
        );
        assert_eq!(
            c.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE name='key_identities'",
                [],
                |r| r.get(0)
            )?,
            i64::from(!corrupt)
        );
        let backup = rusqlite::Connection::open(path.with_extension("db.bak-5"))?;
        assert_eq!(
            backup.pragma_query_value::<u32, _>(None, "user_version", |r| r.get(0))?,
            4
        );
        assert_eq!(
            backup.query_row::<i64, _, _>(
                "SELECT count(*) FROM sqlite_schema WHERE name='key_identities'",
                [],
                |r| r.get(0)
            )?,
            0
        );
    }
    Ok(())
}
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_edit_retains_predicate_credit_until_sql_exits_and_rolls_back() -> TestResult {
    use std::sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
    };
    struct Credit(Arc<AtomicUsize>);
    impl Drop for Credit {
        fn drop(&mut self) {
            self.0.fetch_sub(1, Ordering::SeqCst);
        }
    }
    let dir = private()?;
    let (store, worker) = StateStore::open(dir.path().join("state.db")).await?;
    let vault = initialize(&store).await?;
    let expected = store.load_ssh_key_identities(allowed()).await?;
    let value = identity(vault, "cancelled", Uuid::now_v7(), None)?;
    let outstanding = Arc::new(AtomicUsize::new(1));
    let credit = Credit(outstanding.clone());
    let first = AtomicUsize::new(0);
    let (entered, received) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, blocked) = std::sync::mpsc::channel();
    let blocked = Mutex::new(blocked);
    let authorize = Arc::new(move || {
        let _retained = &credit;
        if first.fetch_add(1, Ordering::SeqCst) == 0 {
            if let Some(sender) = entered.lock().ok().and_then(|mut entry| entry.take()) {
                let _ = sender.send(());
            }
            return blocked.lock().ok().is_some_and(|receiver| {
                receiver
                    .recv_timeout(std::time::Duration::from_secs(3))
                    .is_ok()
            });
        }
        true
    });
    let client = store.clone();
    let task = tokio::spawn(async move {
        client
            .edit_ssh_key_identities(expected, KeyIdentityEdit::Create(value), authorize)
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(2), received).await??;
    task.abort();
    let _ = task.await;
    assert_eq!(outstanding.load(Ordering::SeqCst), 1);
    release.send(())?;
    let snapshot = tokio::time::timeout(
        std::time::Duration::from_secs(3),
        store.load_ssh_key_identities(allowed()),
    )
    .await??;
    assert_eq!(outstanding.load(Ordering::SeqCst), 0);
    assert_eq!(snapshot.revision(), 0);
    assert_eq!(snapshot.records().count(), 0);
    drop(store);
    worker.join().await?;
    Ok(())
}

#[tokio::test]
async fn batch_removal_tombstones_two_roles_without_partial_commit_or_old_source_reuse()
-> TestResult {
    let dir = private()?;
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    let vault = initialize(&store).await?;
    let mut snapshot = store.load_ssh_key_identities(allowed()).await?;
    for name in ["first", "second", "survivor"] {
        snapshot = store
            .edit_ssh_key_identities(
                snapshot,
                KeyIdentityEdit::Create(identity(
                    vault,
                    name,
                    Uuid::now_v7(),
                    Some(Uuid::now_v7()),
                )?),
                allowed(),
            )
            .await?;
    }
    let ids: Vec<_> = snapshot.records().take(2).map(|r| r.id()).collect();
    for invalid in [
        vec![],
        vec![ids[0], ids[0]],
        vec![ids[0], Uuid::now_v7()],
        vec![ids[0]; 257],
    ] {
        assert!(
            store
                .edit_ssh_key_identities(
                    snapshot.clone(),
                    KeyIdentityEdit::RemoveMany(invalid),
                    allowed()
                )
                .await
                .is_err()
        );
        assert_eq!(
            store.load_ssh_key_identities(allowed()).await?.revision(),
            3
        );
    }
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = AtomicUsize::new(0);
    assert!(matches!(
        store
            .edit_ssh_key_identities(
                snapshot.clone(),
                KeyIdentityEdit::RemoveMany(ids.clone()),
                Arc::new(move || calls.fetch_add(1, Ordering::SeqCst) == 0)
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert_eq!(
        store
            .load_ssh_key_identities(allowed())
            .await?
            .records()
            .count(),
        3
    );
    let saved = store
        .edit_ssh_key_identities(
            snapshot.clone(),
            KeyIdentityEdit::RemoveMany(ids.clone()),
            allowed(),
        )
        .await?;
    assert_eq!(saved.revision(), 4);
    assert_eq!(saved.records().count(), 1);
    assert!(
        store
            .edit_ssh_key_identities(snapshot, KeyIdentityEdit::RemoveMany(ids), allowed())
            .await
            .is_err()
    );
    drop(store);
    worker.join().await?;
    let c = crate::database::open(&path)?;
    let (deleted, paired, same_stamp): (i64, i64, bool) = c.query_row("SELECT count(*),count(passphrase_id),min(hlc)=max(hlc) FROM key_identities WHERE deleted=1", [], |r| Ok((r.get(0)?,r.get(1)?,r.get(2)?)))?;
    assert_eq!((deleted, paired), (2, 2));
    assert!(same_stamp);
    drop(c);
    let (store, worker) = StateStore::open(path).await?;
    assert_eq!(
        store
            .load_ssh_key_identities(allowed())
            .await?
            .records()
            .count(),
        1
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
