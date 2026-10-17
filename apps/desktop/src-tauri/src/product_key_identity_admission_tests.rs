use super::*;
use pulse_app::{product_document::ProductDocuments, registry::PageLaunchScope};
use pulse_store::{StateStore, vault_database::VaultEdit, vault_wrap::ValidationWrappedDek};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type PageFixture = (
    SessionRegistry,
    ProductDocuments,
    WindowToken,
    Arc<PageLaunchScope>,
);
fn page() -> std::result::Result<PageFixture, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().map_err(|_| "fixture.window")?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let page = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, page))
}
async fn seed(
    dir: &std::path::Path,
) -> std::result::Result<(StateStore, pulse_store::StateStoreTask), Box<dyn std::error::Error>> {
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    let id = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
    // Valid wire metadata only; these tests never claim successful DEK authentication.
    let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
    store
        .edit_vault_ciphertext(
            empty,
            VaultEdit::Initialize(id, ValidationWrappedDek::decode_ciphertext(&bytes, id)?),
            Arc::new(|| true),
        )
        .await?;
    Ok((store, worker))
}
#[test]
fn key_metadata_wire_cannot_choose_refs_presence_bytes_paths_or_noncanonical_tickets() -> TestResult
{
    for field in [
        "vaultId",
        "secretId",
        "key",
        "privateKey",
        "passphrase",
        "hasValue",
        "path",
        "arguments",
        "with_passphrase",
    ] {
        let value = serde_json::json!({"operation":"create","label":"name","username":"user",field:"forbidden"});
        assert!(serde_json::from_value::<Change>(value).is_err());
    }
    assert!(serde_json::from_value::<Change>(serde_json::json!({"operation":"create","label":"name","username":"user","withPassphrase":"true"})).is_err());
    for value in [
        Uuid::nil().to_string(),
        Uuid::now_v7().to_string(),
        Uuid::new_v4().simple().to_string(),
        Uuid::new_v4().to_string().to_uppercase(),
    ] {
        assert!(ticket(&value).is_err());
    }
    let token = Uuid::new_v4();
    assert_eq!(ticket(&token.to_string())?, token);
    Ok(())
}
#[tokio::test]
async fn key_metadata_native_ticket_crud_refs_presence_conflict_and_reopen() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, worker) = seed(&dir).await?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(profiles::ProfileReader::new());
    let initial = reader
        .key_identities(registry.clone(), page.clone(), dir.clone())
        .await?;
    let bank = metadata_captures::MetadataCaptures::new("workspace.key_identities_failed");
    let token = bank.publish(&registry, page.clone(), initial.clone())?;
    assert!(bank.take(&registry, &page, Uuid::new_v4()).is_err());
    let initial = bank.take(&registry, &page, token)?;
    assert!(bank.take(&registry, &page, token).is_err());
    let saved = reader
        .edit_key_identity(
            registry.clone(),
            page.clone(),
            dir.clone(),
            initial.clone(),
            Change::Create {
                label: "saved-key".into(),
                username: "user".into(),
                with_passphrase: true,
            }
            .edit(&initial)?,
        )
        .await?;
    let row = saved.records().next().ok_or("fixture.row")?;
    let id = row.id();
    let key = row.identity().private_key();
    let pass = row.identity().passphrase().ok_or("fixture.passphrase")?;
    assert_eq!(key.id().get_version_num(), 7);
    assert_eq!(pass.id().get_version_num(), 7);
    assert_ne!(key.id(), pass.id());
    assert_eq!(key.vault_id(), pass.vault_id());
    assert_eq!(key.kind(), SecretKind::SshPrivateKey);
    assert_eq!(pass.kind(), SecretKind::SshKeyPassphrase);
    assert!(!key.has_value());
    assert!(!pass.has_value());
    let dto = serde_json::to_value(projection(Uuid::new_v4(), saved.clone()))?;
    assert_eq!(dto["vaultPresent"], true);
    assert_eq!(dto["identities"][0]["secret"]["hasValue"], false);
    assert_eq!(dto["identities"][0]["passphrase"]["hasValue"], false);
    assert!(dto["identities"][0]["secret"].get("vaultId").is_none());
    assert!(dto.to_string().find("ciphertext").is_none());
    let updated = reader
        .edit_key_identity(
            registry.clone(),
            page.clone(),
            dir.clone(),
            saved.clone(),
            Change::Update {
                id: id.to_string(),
                label: "updated".into(),
                username: "next-user".into(),
            }
            .edit(&saved)?,
        )
        .await?;
    assert!(matches!(
        reader
            .edit_key_identity(
                registry.clone(),
                page.clone(),
                dir.clone(),
                saved,
                KeyIdentityEdit::Remove(id)
            )
            .await,
        Err("workspace.key_identities_conflict")
    ));
    let value = updated.records().next().ok_or("fixture.row")?.identity();
    assert_eq!(value.private_key().id(), key.id());
    assert_eq!(value.passphrase().map(|value| value.id()), Some(pass.id()));
    let removed = reader
        .edit_key_identity(
            registry.clone(),
            page.clone(),
            dir.clone(),
            updated.clone(),
            Change::Remove { id: id.to_string() }.edit(&updated)?,
        )
        .await?;
    assert_eq!(removed.records().count(), 0);
    assert!(matches!(
        reader
            .edit_key_identity(
                registry.clone(),
                page,
                dir.clone(),
                updated,
                KeyIdentityEdit::Remove(id)
            )
            .await,
        Err("workspace.key_identities_conflict")
    ));
    reader.shutdown().await?;
    drop(store);
    worker.join().await?;
    registry.shutdown()?;
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    let reopened = store.load_ssh_key_identities(Arc::new(|| true)).await?;
    assert_eq!(reopened.revision(), 3);
    assert_eq!(reopened.records().count(), 0);
    drop(store);
    worker.join().await?;
    Ok(())
}
#[tokio::test]
async fn key_metadata_navigation_rejects_queued_read_and_old_expired_or_closed_capture()
-> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("state");
    let (registry, documents, owner, page) = page()?;
    let reader = Arc::new(profiles::ProfileReader::new());
    let pending = reader.key_identities(registry.clone(), page.clone(), dir.clone());
    tokio::pin!(pending);
    std::future::poll_fn(|cx| {
        use std::future::Future;
        match pending.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(_) => panic!("fixture.must_queue"),
        }
    })
    .await;
    documents.started(owner)?;
    assert!(matches!(pending.await, Err("workspace.denied")));
    assert!(!dir.exists());
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let current = documents.current(owner, nonce)?;
    let snapshot = reader
        .key_identities(registry.clone(), current.clone(), dir)
        .await?;
    assert!(snapshot.vault_id().is_none());
    assert!(matches!(
        Change::Create {
            label: "no-vault".into(),
            username: "user".into(),
            with_passphrase: false
        }
        .edit(&snapshot),
        Err("workspace.vault_missing")
    ));
    let bank = metadata_captures::MetadataCaptures::new("workspace.key_identities_failed");
    assert!(bank.publish(&registry, page, snapshot.clone()).is_err());
    let token = bank.publish(&registry, current.clone(), snapshot.clone())?;
    bank.expire_for_test()?;
    assert!(bank.take(&registry, &current, token).is_err());
    let token = bank.publish(&registry, current.clone(), snapshot.clone())?;
    registry.close_window(owner)?;
    bank.retire();
    assert!(bank.take(&registry, &current, token).is_err());
    assert!(bank.publish(&registry, current, snapshot).is_err());
    reader.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn key_metadata_foreign_registry_store_instance_and_directory_replacement_reject()
-> TestResult {
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, worker) = seed(&dir).await?;
    let (registry, _documents, _owner, page) = page()?;
    let reader = Arc::new(profiles::ProfileReader::new());
    let initial = reader
        .key_identities(registry.clone(), page.clone(), dir.clone())
        .await?;
    let (foreign_registry, _documents, _owner, _page) = self::page()?;
    assert!(matches!(
        reader
            .key_identities(foreign_registry.clone(), page.clone(), dir.clone())
            .await,
        Err("workspace.denied")
    ));
    let foreign_snapshot = store.load_ssh_key_identities(Arc::new(|| true)).await?;
    assert!(matches!(
        reader
            .edit_key_identity(
                registry.clone(),
                page.clone(),
                dir.clone(),
                foreign_snapshot,
                Change::Create {
                    label: "foreign".into(),
                    username: "user".into(),
                    with_passphrase: false
                }
                .edit(&initial)?
            )
            .await,
        Err("workspace.key_identities_conflict")
    ));
    let replacement = root.path().join("replacement");
    assert!(matches!(
        reader
            .key_identities(registry.clone(), page, replacement.clone())
            .await,
        Err("workspace.denied")
    ));
    assert!(!replacement.exists());
    reader.shutdown().await?;
    drop(store);
    worker.join().await?;
    foreign_registry.shutdown()?;
    registry.shutdown()?;
    Ok(())
}
