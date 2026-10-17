use super::*;
#[path = "ssh_route_agent_source_tests.rs"]
mod agent;
#[path = "ssh_alias_source_tests.rs"]
mod alias;
#[path = "ssh_route_hop_source_tests.rs"]
mod hop;
use crate::{
    database,
    ssh_hosts::{self, HostEdit},
    ssh_identities::{self, IdentityEdit},
    ssh_key_identities::{self, KeyIdentityEdit},
    ssh_profiles::{self, ProfileEdit, ProfileMutation},
    vault_database::{self, VaultEdit},
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
    ssh_key_identity::SshPrivateKeyIdentity,
};
use std::sync::{
    Condvar, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Fixture {
    dir: tempfile::TempDir,
    db: Connection,
    owner: Uuid,
    profiles: [Uuid; 3],
    entrances: [Uuid; 3],
    password: Uuid,
    key: Uuid,
}
fn fixture() -> std::result::Result<Fixture, Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let db = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
    // Shape-only encrypted envelope. No DEK or credentials are activated in this batch.
    let mut envelope = vec![0; 110];
    envelope[..6].copy_from_slice(b"PVDE\x01\x01");
    envelope[6..22].copy_from_slice(vault.as_bytes());
    vault_database::edit(
        &db,
        owner,
        &vault_database::load(&db, owner)?,
        VaultEdit::Initialize(
            vault,
            ValidationWrappedDek::decode_ciphertext(&envelope, vault)?,
        ),
        || true,
    )?;
    ssh_identities::edit_at(
        &db,
        owner,
        &ssh_identities::load(&db, owner)?,
        IdentityEdit::Create(SshPasswordIdentity::new(
            "route-password",
            "route-user",
            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
        )?),
        100,
        || true,
    )?;
    ssh_key_identities::edit(
        &db,
        owner,
        &ssh_key_identities::load(&db, owner)?,
        KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
            "route-key",
            "route-user",
            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
            None,
        )?),
        || true,
    )?;
    let password = ssh_identities::load(&db, owner)?
        .records()
        .next()
        .ok_or("fixture.password")?
        .id();
    let key = ssh_key_identities::load(&db, owner)?
        .records()
        .next()
        .ok_or("fixture.key")?
        .id();
    let labels = ["outer", "middle", "destination"];
    for label in labels {
        ssh_hosts::edit(
            &db,
            owner,
            &ssh_hosts::load(&db, owner)?,
            HostEdit::Create(SshEndpoint::new(
                label,
                &format!("{label}.invalid"),
                22,
                "route-user",
            )?),
            || true,
        )?;
    }
    let snapshot = ssh_profiles::load(&db, owner)?;
    let mut profiles = [Uuid::nil(); 3];
    let mut entrances = [Uuid::nil(); 3];
    for (i, label) in labels.iter().enumerate() {
        let profile = snapshot
            .profiles()
            .find(|p| p.label() == *label)
            .ok_or("fixture.profile")?;
        profiles[i] = profile.metadata().id();
        entrances[i] = profile.endpoint_id();
    }
    for (i, label) in labels.iter().enumerate() {
        let expected = ssh_profiles::load(&db, owner)?;
        ssh_profiles::edit(
            &db,
            owner,
            &expected,
            ProfileMutation::Complete(
                ProfileEdit {
                    id: profiles[i],
                    label: label.to_string(),
                    username: "route-user".to_owned(),
                    identity: Some(if i == 1 { key } else { password }),
                },
                i.checked_sub(1).map(|n| profiles[n]),
            ),
            || true,
        )?;
    }
    Ok(Fixture {
        dir,
        db,
        owner,
        profiles,
        entrances,
        password,
        key,
    })
}
fn app_state(db: &Connection) -> std::result::Result<Vec<(String, String)>, rusqlite::Error> {
    let mut q = db.prepare("SELECT key,value FROM app_state ORDER BY key")?;
    q.query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?.collect()
}

#[tokio::test]
async fn mixed_route_is_one_read_only_worker_source_and_keeps_direct_guards_closed() -> TestResult {
    let f = fixture()?;
    let before = app_state(&f.db)?;
    let (store, task) = StateStore::open(f.dir.path().join("state.db")).await?;
    let source = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    assert_eq!(source.hops().len(), 3);
    assert_eq!(
        source
            .hops()
            .iter()
            .map(SshRouteHop::profile_id)
            .collect::<Vec<_>>(),
        f.profiles
    );
    assert_eq!(
        source
            .hops()
            .iter()
            .map(SshRouteHop::entrance_id)
            .collect::<Vec<_>>(),
        f.entrances
    );
    assert!(source.hops()[0].identity() == SshRouteIdentity::Password(f.password));
    assert!(source.hops()[1].identity() == SshRouteIdentity::PrivateKey(f.key));
    assert!(source.hops()[2].identity() == SshRouteIdentity::Password(f.password));
    assert_eq!(
        source.hops()[1].endpoint().target().host(),
        "middle.invalid"
    );
    assert!(
        !source
            .target_source()
            .hosts()
            .supports_direct_ssh(f.entrances[1])
    );
    assert!(
        !source
            .target_source()
            .hosts()
            .supports_direct_ssh(f.entrances[2])
    );
    // A captured kind/reference is not an available credential or a permission to unlock.
    assert!(
        !source
            .target_source()
            .identities()
            .records()
            .next()
            .ok_or("fixture.key")?
            .identity()
            .private_key()
            .has_value()
    );
    store
        .validate_ssh_route_source(source.clone(), Arc::new(|| true))
        .await?;
    let explicit_leaf = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::PrivateKey(f.key),
            Arc::new(|| true),
        )
        .await?;
    assert!(explicit_leaf.hops()[2].identity() == SshRouteIdentity::PrivateKey(f.key));
    assert!(
        store
            .load_ssh_route_source(
                f.profiles[0],
                SshRouteIdentity::Password(f.password),
                Arc::new(|| true)
            )
            .await
            .is_err()
    );
    drop(store);
    task.shutdown().await?;
    assert_eq!(app_state(&f.db)?, before);
    // A new store instance cannot reuse the previous source, even for the exact same bytes.
    let (reopened, task) = StateStore::open(f.dir.path().join("state.db")).await?;
    assert!(matches!(
        reopened
            .validate_ssh_route_source(source, Arc::new(|| true))
            .await,
        Err(StoreError::IdentityConflict)
    ));
    assert_eq!(
        reopened
            .load_ssh_route_source(
                f.profiles[2],
                SshRouteIdentity::Password(f.password),
                Arc::new(|| true)
            )
            .await?
            .hops()
            .len(),
        3
    );
    drop(reopened);
    task.shutdown().await?;
    Ok(())
}

#[test]
fn frozen_route_resolution_uses_only_original_snapshot_and_rejects_foreign_owner() -> TestResult {
    let f = fixture()?;
    let raw = load(
        &f.db,
        f.owner,
        f.profiles[2],
        SshRouteIdentity::Password(f.password),
    )?
    .source;
    let before = app_state(&f.db)?;
    let route = resolve(
        f.owner,
        raw.clone(),
        f.profiles[2],
        SshRouteIdentity::Password(f.password),
    )?;
    assert_eq!(route.hops().len(), 3);
    assert!(
        resolve(
            Uuid::new_v4(),
            raw.clone(),
            f.profiles[2],
            SshRouteIdentity::Password(f.password)
        )
        .is_err()
    );
    assert_eq!(app_state(&f.db)?, before);
    f.db.execute("DELETE FROM app_state", [])?;
    assert_eq!(
        resolve(
            f.owner,
            raw,
            f.profiles[2],
            SshRouteIdentity::Password(f.password)
        )?
        .hops()
        .len(),
        3
    );
    assert!(
        load(
            &f.db,
            f.owner,
            f.profiles[2],
            SshRouteIdentity::Password(f.password)
        )
        .is_err()
    );
    Ok(())
}

#[test]
fn source_rejects_type_ambiguity_same_revision_edits_missing_defaults_and_stale_edges() -> TestResult
{
    let f = fixture()?;
    let identity = SshRouteIdentity::Password(f.password);
    let expected = load(&f.db, f.owner, f.profiles[2], identity)?;
    assert!(matches!(
        load(
            &f.db,
            f.owner,
            f.profiles[2],
            SshRouteIdentity::PrivateKey(f.password)
        ),
        Err(StoreError::IdentityConflict)
    ));
    let revision = ssh_key_identities::load(&f.db, f.owner)?.revision();
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["replaced metadata", f.key.to_string()],
    )?;
    assert_eq!(
        ssh_key_identities::load(&f.db, f.owner)?.revision(),
        revision
    );
    assert!(matches!(
        validate(&f.db, f.owner, &expected, || true),
        Err(StoreError::IdentityConflict)
    ));
    f.db.execute(
        "UPDATE key_identities SET label=? WHERE id=?",
        rusqlite::params!["route-key", f.key.to_string()],
    )?;
    // Imported IDs cannot silently choose between password and private-key directories.
    f.db.execute(
        "UPDATE key_identities SET id=? WHERE id=?",
        rusqlite::params![f.password.to_string(), f.key.to_string()],
    )?;
    assert!(load(&f.db, f.owner, f.profiles[2], identity).is_err());
    f.db.execute(
        "UPDATE key_identities SET id=? WHERE id=?",
        rusqlite::params![f.key.to_string(), f.password.to_string()],
    )?;
    f.db.execute(
        "UPDATE identities SET username=? WHERE id=?",
        rusqlite::params!["different-user", f.password.to_string()],
    )?;
    assert!(matches!(
        load(&f.db, f.owner, f.profiles[2], identity),
        Err(StoreError::IdentityConflict)
    ));
    f.db.execute(
        "UPDATE identities SET username=? WHERE id=?",
        rusqlite::params!["route-user", f.password.to_string()],
    )?;
    let profile = ssh_profiles::load(&f.db, f.owner)?;
    ssh_profiles::edit(
        &f.db,
        f.owner,
        &profile,
        ProfileMutation::Complete(
            ProfileEdit {
                id: f.profiles[1],
                label: "middle".to_owned(),
                username: "route-user".to_owned(),
                identity: None,
            },
            Some(f.profiles[0]),
        ),
        || true,
    )?;
    assert!(matches!(
        load(&f.db, f.owner, f.profiles[2], identity),
        Err(StoreError::IdentityConflict)
    ));
    assert!(validate(&f.db, f.owner, &expected, || true).is_err());
    let current = ssh_profiles::load(&f.db, f.owner)?;
    ssh_profiles::edit(
        &f.db,
        f.owner,
        &current,
        ProfileMutation::Jump {
            id: f.profiles[2],
            via: None,
        },
        || true,
    )?;
    assert!(matches!(
        load(&f.db, f.owner, f.profiles[2], identity),
        Err(StoreError::HostConflict)
    ));
    Ok(())
}

struct Credit(Arc<AtomicBool>);
impl Credit {
    fn held(&self) -> bool {
        !self.0.load(Ordering::SeqCst)
    }
}
impl Drop for Credit {
    fn drop(&mut self) {
        self.0.store(true, Ordering::SeqCst);
    }
}
struct Release(Arc<(Mutex<bool>, Condvar)>);
impl Release {
    fn open(&self) {
        *self.0.0.lock().unwrap_or_else(|e| e.into_inner()) = true;
        self.0.1.notify_all();
    }
}
impl Drop for Release {
    fn drop(&mut self) {
        self.open();
    }
}

#[tokio::test]
async fn final_revocation_panic_and_cancelled_waiter_retain_credit_until_worker_exit() -> TestResult
{
    let f = fixture()?;
    let before = app_state(&f.db)?;
    let (store, task) = StateStore::open(f.dir.path().join("state.db")).await?;
    let calls = Arc::new(AtomicUsize::new(0));
    let final_calls = calls.clone();
    assert!(matches!(
        store
            .load_ssh_route_source(
                f.profiles[2],
                SshRouteIdentity::Password(f.password),
                Arc::new(move || final_calls.fetch_add(1, Ordering::SeqCst) == 0)
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    assert!(calls.load(Ordering::SeqCst) >= 2);
    assert!(matches!(
        store
            .load_ssh_route_source(
                f.profiles[2],
                SshRouteIdentity::Password(f.password),
                Arc::new(|| panic!("synthetic admission"))
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let source = store
        .load_ssh_route_source(
            f.profiles[2],
            SshRouteIdentity::Password(f.password),
            Arc::new(|| true),
        )
        .await?;
    let calls = Arc::new(AtomicUsize::new(0));
    assert!(matches!(
        store
            .validate_ssh_route_source(
                source,
                Arc::new(move || calls.fetch_add(1, Ordering::SeqCst) == 0)
            )
            .await,
        Err(StoreError::IdentityAuthorization)
    ));
    let dropped = Arc::new(AtomicBool::new(false));
    let credit = Credit(dropped.clone());
    let release = Release(Arc::new((Mutex::new(false), Condvar::new())));
    let gate = release.0.clone();
    let (started, waiting) = oneshot::channel();
    let started = Mutex::new(Some(started));
    let caller = store.clone();
    let destination = f.profiles[2];
    let identity = SshRouteIdentity::Password(f.password);
    let pending = tokio::spawn(async move {
        caller
            .load_ssh_route_source(
                destination,
                identity,
                Arc::new(move || {
                    let retained = credit.held();
                    if let Some(signal) = started.lock().unwrap_or_else(|e| e.into_inner()).take() {
                        let _ = signal.send(());
                    }
                    let mut open = gate.0.lock().unwrap_or_else(|e| e.into_inner());
                    while !*open {
                        open = gate.1.wait(open).unwrap_or_else(|e| e.into_inner());
                    }
                    retained
                }),
            )
            .await
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), waiting).await??;
    pending.abort();
    assert!(pending.await.is_err());
    assert!(!dropped.load(Ordering::SeqCst));
    release.open();
    // FIFO worker admission confirms the preceding operation and its predicate have really exited.
    store
        .load_ssh_route_source(destination, identity, Arc::new(|| true))
        .await?;
    assert!(dropped.load(Ordering::SeqCst));
    drop(store);
    task.shutdown().await?;
    assert_eq!(app_state(&f.db)?, before);
    Ok(())
}
