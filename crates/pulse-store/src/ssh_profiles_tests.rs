use super::*;
use crate::{
    database,
    vault_database::{self, VaultEdit},
    vault_wrap::ValidationWrappedDek,
};
use pulse_core::{
    secret_ref::{SecretKind, SecretRef},
    ssh_identity::SshPasswordIdentity,
    ssh_key_identity::SshPrivateKeyIdentity,
};
use std::sync::atomic::{AtomicUsize, Ordering};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn fixture()
-> std::result::Result<(tempfile::TempDir, Connection, Uuid), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let db = database::open(&dir.path().join("state.db"))?;
    let owner = Uuid::new_v4();
    let vault = Uuid::now_v7();
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
        ssh_identities::IdentityEdit::Create(SshPasswordIdentity::new(
            "shared login",
            "fixed-user",
            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
        )?),
        100,
        || true,
    )?;
    ssh_key_identities::edit(
        &db,
        owner,
        &ssh_key_identities::load(&db, owner)?,
        ssh_key_identities::KeyIdentityEdit::Create(SshPrivateKeyIdentity::new(
            "shared key",
            "fixed-user",
            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPrivateKey, false)?,
            None,
        )?),
        || true,
    )?;
    ssh_hosts::edit(
        &db,
        owner,
        &ssh_hosts::load(&db, owner)?,
        HostEdit::Create(SshEndpoint::new(
            "SSH entrance",
            "fixture.invalid",
            22,
            "fixed-user",
        )?),
        || true,
    )?;
    Ok((dir, db, owner))
}
fn change(
    source: &ProfileSnapshot,
    identity: Option<Uuid>,
) -> std::result::Result<ProfileEdit, Box<dyn std::error::Error>> {
    Ok(ProfileEdit {
        id: source
            .profiles()
            .next()
            .ok_or("fixture.profile")?
            .metadata()
            .id(),
        label: "operator preferences".into(),
        username: "fixed-user".into(),
        identity,
    })
}
fn host_rows(db: &Connection) -> std::result::Result<Vec<(String, String)>, rusqlite::Error> {
    db.prepare("SELECT key,value FROM app_state ORDER BY key")?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}
#[tokio::test]
async fn worker_reopen_independent_profile_name_and_both_references_never_create_credentials()
-> TestResult {
    let (dir, db, _) = fixture()?;
    drop(db);
    let path = dir.path().join("state.db");
    let (store, worker) = StateStore::open(path.clone()).await?;
    assert!(matches!(
        store.load_ssh_profiles_authorized(Arc::new(|| false)).await,
        Err(StoreError::HostAuthorization)
    ));
    let source = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let id = source
        .password_identities()
        .next()
        .ok_or("fixture.identity")?
        .id();
    let edited = store
        .edit_ssh_profile(
            source.clone(),
            change(&source, Some(id))?,
            Arc::new(|| true),
        )
        .await?;
    let profile_id = edited
        .profiles()
        .next()
        .ok_or("fixture.profile")?
        .metadata()
        .id();
    assert!(edited.passwords == source.passwords && edited.keys == source.keys);
    assert_eq!(
        edited.hosts().records()[0].endpoint().label(),
        "SSH entrance"
    );
    assert_eq!(
        edited.profiles().next().ok_or("fixture.profile")?.label(),
        "operator preferences"
    );
    assert_eq!(
        edited
            .profiles()
            .next()
            .ok_or("fixture.profile")?
            .identity_id(),
        Some(id)
    );
    assert!(
        !edited
            .password_identities()
            .next()
            .ok_or("fixture.identity")?
            .identity()
            .password()
            .has_value()
    );
    let entrance = &edited.hosts().records()[0];
    store
        .edit_ssh_hosts(
            edited.hosts().clone(),
            HostEdit::Update(
                entrance.id(),
                SshEndpoint::new("renamed entrance", "fixture.invalid", 2222, "fixed-user")?,
            ),
            Arc::new(|| true),
        )
        .await?;
    let next = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    assert_eq!(
        next.profiles().next().ok_or("fixture.profile")?.label(),
        "operator preferences"
    );
    assert_eq!(
        next.profiles()
            .next()
            .ok_or("fixture.profile")?
            .identity_id(),
        Some(id)
    );
    let key = next.key_identities().next().ok_or("fixture.key")?.id();
    let saved = store
        .edit_ssh_profile(next.clone(), change(&next, Some(key))?, Arc::new(|| true))
        .await?;
    assert!(saved.passwords == next.passwords && saved.keys == next.keys);
    assert!(
        !saved
            .key_identities()
            .next()
            .ok_or("fixture.key")?
            .identity()
            .private_key()
            .has_value()
    );
    drop(store);
    worker.join().await?;
    let (store, worker) = StateStore::open(path).await?;
    let reopened = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let profile = reopened.profiles().next().ok_or("fixture.profile")?;
    assert_eq!(profile.metadata().id(), profile_id);
    assert_eq!(profile.identity_id(), Some(key));
    assert_eq!(profile.label(), "operator preferences");
    assert_eq!(
        reopened.hosts().records()[0].endpoint().label(),
        "renamed entrance"
    );
    drop(store);
    worker.join().await?;
    Ok(())
}
#[test]
fn same_revision_identity_change_deleted_identity_foreign_source_and_unknown_reference_are_rejected()
-> TestResult {
    let (_dir, db, owner) = fixture()?;
    let source = load(&db, owner)?;
    let before = host_rows(&db)?;
    for id in [Uuid::now_v7(), source.hosts().records()[0].id()] {
        assert!(matches!(
            edit(&db, owner, &source, change(&source, Some(id))?, || true),
            Err(StoreError::HostRecord)
        ));
    }
    assert!(matches!(
        edit(&db, Uuid::new_v4(), &source, change(&source, None)?, || {
            true
        }),
        Err(StoreError::HostConflict)
    ));
    let identity = source
        .password_identities()
        .next()
        .ok_or("fixture.identity")?
        .id();
    db.execute(
        "UPDATE identities SET label='changed metadata' WHERE id=?1",
        [identity.to_string()],
    )?;
    assert!(matches!(
        edit(
            &db,
            owner,
            &source,
            change(&source, Some(identity))?,
            || true
        ),
        Err(StoreError::HostConflict)
    ));
    assert_eq!(host_rows(&db)?, before);
    let refreshed = load(&db, owner)?;
    let key = refreshed.key_identities().next().ok_or("fixture.key")?.id();
    ssh_key_identities::edit(
        &db,
        owner,
        &refreshed.keys,
        ssh_key_identities::KeyIdentityEdit::Remove(key),
        || true,
    )?;
    assert!(matches!(
        edit(
            &db,
            owner,
            &refreshed,
            change(&refreshed, Some(key))?,
            || true
        ),
        Err(StoreError::HostConflict)
    ));
    let after_delete = load(&db, owner)?;
    assert!(matches!(
        edit(
            &db,
            owner,
            &after_delete,
            change(&after_delete, Some(key))?,
            || true
        ),
        Err(StoreError::HostRecord)
    ));
    assert_eq!(host_rows(&db)?, before);
    Ok(())
}
#[test]
fn final_revocation_rolls_back_both_graph_and_legacy_projection_and_stale_entrance_cannot_save()
-> TestResult {
    let (_dir, db, owner) = fixture()?;
    let source = load(&db, owner)?;
    let before = host_rows(&db)?;
    let calls = AtomicUsize::new(0);
    let identity = source
        .password_identities()
        .next()
        .ok_or("fixture.identity")?
        .id();
    let mut proposal = change(&source, Some(identity))?;
    proposal.username = "explicit-user".into();
    assert!(matches!(
        edit(&db, owner, &source, proposal, || calls
            .fetch_add(1, Ordering::SeqCst)
            < 3),
        Err(StoreError::HostAuthorization)
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 4);
    assert_eq!(host_rows(&db)?, before);
    ssh_hosts::edit(
        &db,
        owner,
        source.hosts(),
        HostEdit::Remove(source.hosts().records()[0].id()),
        || true,
    )?;
    assert!(matches!(
        edit(&db, owner, &source, change(&source, None)?, || true),
        Err(StoreError::HostConflict)
    ));
    Ok(())
}
#[test]
fn agent_metadata_invalidates_common_source_and_profile_cas_and_referenced_delete() -> TestResult {
    use crate::ssh_agent_identities::{self as agents, AgentIdentityEdit};
    let (_dir, db, owner) = fixture()?;
    let old = load(&db, owner)?;
    #[cfg(feature = "ssh-trust")]
    let common = crate::ssh_target_source::read(&db, owner)?;
    let unchanged = host_rows(&db)?;
    let created = agents::edit(
        &db,
        owner,
        &agents::load(&db, owner)?,
        AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
            "Selected Agent",
            "fixed-user",
            "ssh-ed25519",
            &format!("SHA256:{}", "A".repeat(43)),
        )?),
        || true,
    )?;
    let id = created.records().next().ok_or("fixture.agent")?.id();
    #[cfg(feature = "ssh-trust")]
    assert!(!crate::ssh_target_source::same(
        &common,
        &crate::ssh_target_source::read(&db, owner)?
    )?);
    assert!(matches!(
        edit(&db, owner, &old, change(&old, None)?, || true),
        Err(StoreError::HostConflict)
    ));
    assert_eq!(host_rows(&db)?, unchanged);
    let before_replacement = load(&db, owner)?;
    #[cfg(feature = "ssh-trust")]
    let common = crate::ssh_target_source::read(&db, owner)?;
    db.execute(
        "UPDATE agent_identities SET label='replaced at same revision' WHERE id=?1",
        [id.to_string()],
    )?;
    assert_eq!(agents::load(&db, owner)?.revision(), created.revision());
    #[cfg(feature = "ssh-trust")]
    assert!(!crate::ssh_target_source::same(
        &common,
        &crate::ssh_target_source::read(&db, owner)?
    )?);
    assert!(matches!(
        edit(
            &db,
            owner,
            &before_replacement,
            change(&before_replacement, Some(id))?,
            || true
        ),
        Err(StoreError::HostConflict)
    ));
    assert_eq!(host_rows(&db)?, unchanged);
    let fresh = load(&db, owner)?;
    let selected = edit(&db, owner, &fresh, change(&fresh, Some(id))?, || true)?;
    assert_eq!(
        selected
            .profiles()
            .next()
            .ok_or("fixture.profile")?
            .identity_id(),
        Some(id)
    );
    let current = agents::load(&db, owner)?;
    assert!(matches!(
        agents::edit(&db, owner, &current, AgentIdentityEdit::Remove(id), || true),
        Err(StoreError::IdentityConflict)
    ));
    let released = edit(&db, owner, &selected, change(&selected, None)?, || true)?;
    agents::edit(&db, owner, &current, AgentIdentityEdit::Remove(id), || true)?;
    assert!(matches!(
        edit(&db, owner, &released, change(&released, None)?, || true),
        Err(StoreError::HostConflict)
    ));
    assert_eq!(agents::load(&db, owner)?.records().count(), 0);
    Ok(())
}
#[test]
fn agent_identity_id_collision_cannot_select_password_or_save_ambiguous_profile() -> TestResult {
    use crate::ssh_agent_identities::{self as agents, AgentIdentityEdit};
    let (_dir, db, owner) = fixture()?;
    let original = load(&db, owner)?;
    let password = original
        .password_identities()
        .next()
        .ok_or("fixture.password")?
        .id();
    let created = agents::edit(
        &db,
        owner,
        &agents::load(&db, owner)?,
        AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
            "Other type",
            "fixed-user",
            "ssh-ed25519",
            &format!("SHA256:{}", "A".repeat(43)),
        )?),
        || true,
    )?;
    let agent = created.records().next().ok_or("fixture.agent")?.id();
    db.execute(
        "UPDATE agent_identities SET id=?1 WHERE id=?2",
        [password.to_string(), agent.to_string()],
    )?;
    let current = load(&db, owner)?;
    #[cfg(feature = "ssh-trust")]
    let common = crate::ssh_target_source::read(&db, owner)?;
    #[cfg(feature = "ssh-trust")]
    assert!(!common.supports_password_selection(current.hosts().records()[0].id(), password));
    let rows = host_rows(&db)?;
    assert!(matches!(
        edit(
            &db,
            owner,
            &current,
            change(&current, Some(password))?,
            || true
        ),
        Err(StoreError::HostRecord)
    ));
    assert_eq!(host_rows(&db)?, rows);
    Ok(())
}
#[path = "ssh_jump_store_tests.rs"]
mod jump;
