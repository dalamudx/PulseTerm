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
    let r = SessionRegistry::new();
    let d = ProductDocuments::new(r.clone());
    let owner = r.open_window().map_err(|_| "fixture.window")?;
    d.install(owner)?;
    d.started(owner)?;
    let nonce = d.finished(owner)?;
    d.confirm(owner, nonce)?;
    let p = d.current(owner, nonce)?;
    Ok((r, d, owner, p))
}
async fn seed(
    dir: &std::path::Path,
) -> std::result::Result<(StateStore, pulse_store::StateStoreTask), Box<dyn std::error::Error>> {
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    let id = Uuid::now_v7();
    let mut bytes = vec![0; 110];
    bytes[..6].copy_from_slice(b"PVDE\x01\x01");
    bytes[6..22].copy_from_slice(id.as_bytes());
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
fn wire_change_rejects_raw_ref_password_flags_and_noncanonical_tickets() -> TestResult {
    for field in [
        "vaultId",
        "secretId",
        "password",
        "hasValue",
        "path",
        "arguments",
    ] {
        let value = serde_json::json!({"operation":"create","label":"name","username":"user",field:"not-allowed"});
        assert!(serde_json::from_value::<Change>(value).is_err());
    }
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
async fn native_same_source_backend_reservation_exact_ticket_and_stale_edit() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, worker) = seed(&dir).await?;
    let (r, _d, _owner, p) = page()?;
    let reader = Arc::new(profiles::ProfileReader::new());
    let initial = reader.identities(r.clone(), p.clone(), dir.clone()).await?;
    assert!(initial.vault_id().is_some());
    let bank = metadata_captures::MetadataCaptures::new("workspace.identities_failed");
    let token = bank.publish(&r, p.clone(), initial.clone())?;
    assert!(bank.take(&r, &p, Uuid::new_v4()).is_err());
    let initial = bank.take(&r, &p, token)?;
    assert!(bank.take(&r, &p, token).is_err());
    let edit = Change::Create {
        label: "native-name".into(),
        username: "fixed-user".into(),
    }
    .edit(&initial)?;
    let saved = reader
        .edit_identity(r.clone(), p.clone(), dir.clone(), initial, edit)
        .await?;
    let row = saved.records().next().ok_or("fixture.row")?;
    let id = row.id();
    let secret = row.identity().password();
    assert_eq!(secret.id().get_version_num(), 7);
    assert!(!secret.has_value());
    assert_eq!(saved.records().count(), 1);
    let dto = serde_json::to_value(projection(Uuid::new_v4(), saved.clone()))?;
    assert_eq!(dto["vaultPresent"], true);
    assert_eq!(dto["identities"][0]["secret"]["hasValue"], false);
    assert!(dto["identities"][0]["secret"].get("vaultId").is_none());
    assert!(dto.to_string().find("ciphertext").is_none());
    let updated = reader
        .edit_identity(
            r.clone(),
            p.clone(),
            dir.clone(),
            saved.clone(),
            Change::Update {
                id: id.to_string(),
                label: "updated".into(),
                username: "fixed-user".into(),
            }
            .edit(&saved)?,
        )
        .await?;
    assert!(matches!(
        reader
            .edit_identity(r.clone(), p.clone(), dir, saved, IdentityEdit::Remove(id))
            .await,
        Err("workspace.identities_conflict")
    ));
    assert_eq!(
        updated
            .records()
            .next()
            .ok_or("fixture.row")?
            .identity()
            .password()
            .id(),
        secret.id()
    );
    reader.shutdown().await?;
    assert_eq!(
        store
            .load_ssh_identities(Arc::new(|| true))
            .await?
            .revision(),
        2
    );
    drop(store);
    worker.join().await?;
    r.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn navigation_cancels_queued_source_no_default_vault_and_late_capture_cannot_publish()
-> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("state");
    let (r, d, owner, p) = page()?;
    let reader = Arc::new(profiles::ProfileReader::new());
    let pending = reader.identities(r.clone(), p.clone(), dir.clone());
    tokio::pin!(pending);
    std::future::poll_fn(|cx| {
        use std::future::Future;
        match pending.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(_) => panic!("fixture.must_queue"),
        }
    })
    .await;
    d.started(owner)?;
    assert!(matches!(pending.await, Err("workspace.denied")));
    assert!(!dir.exists());
    let nonce = d.finished(owner)?;
    d.confirm(owner, nonce)?;
    let current = d.current(owner, nonce)?;
    let snapshot = reader
        .identities(r.clone(), current.clone(), dir.clone())
        .await?;
    assert!(snapshot.vault_id().is_none());
    assert!(matches!(
        Change::Create {
            label: "no-vault".into(),
            username: "user".into()
        }
        .edit(&snapshot),
        Err("workspace.vault_missing")
    ));
    let bank = metadata_captures::MetadataCaptures::new("workspace.identities_failed");
    assert!(bank.publish(&r, p, snapshot.clone()).is_err());
    let token = bank.publish(&r, current.clone(), snapshot.clone())?;
    bank.expire_for_test()?;
    assert!(bank.take(&r, &current, token).is_err());
    let token = bank.publish(&r, current.clone(), snapshot.clone())?;
    d.started(owner)?;
    bank.retire();
    assert!(bank.take(&r, &current, token).is_err());
    assert!(bank.publish(&r, current, snapshot).is_err());
    reader.shutdown().await?;
    r.shutdown()?;
    Ok(())
}

#[tokio::test]
#[ignore = "fixed private metadata fixture, explicitly invoked by real-window parent probe"]
async fn identity_window_fixture() -> TestResult {
    let dir = std::path::PathBuf::from(std::env::var("PULSE_IDENTITY_FIXTURE_DIRECTORY")?);
    let base = dir
        .parent()
        .and_then(|p| p.parent())
        .ok_or("fixture.path")?;
    if !dir.is_absolute()
        || !dir.starts_with("/data/temp")
        || dir.file_name().and_then(|n| n.to_str()) != Some("dev.pulseterm.workspace-preview")
        || !base
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with("workspace-identities."))
    {
        return Err("fixture.path".into());
    }
    let dir = pulse_store::state_directory::prepare(&dir)?;
    let (store, worker) = seed(&dir).await?;
    assert!(
        store
            .load_ssh_identities(Arc::new(|| true))
            .await?
            .records()
            .next()
            .is_none()
    );
    drop(store);
    worker.join().await?;
    println!("workspace.identity_metadata_only_fixture_confirmed");
    Ok(())
}
