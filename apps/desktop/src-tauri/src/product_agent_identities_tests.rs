use super::*;
use pulse_app::product_document::ProductDocuments;
type TestResult<T = ()> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
type PageFixture = (
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<pulse_app::registry::PageLaunchScope>,
);
fn page() -> TestResult<PageFixture> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let page = documents.current(owner, nonce)?;
    Ok((registry, documents, owner, page))
}
#[tokio::test]
async fn original_page_agent_drafts_reject_foreign_page_retired_read_and_cancelled_save_keeps_busy_until_guard_drop()
-> TestResult {
    let root = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let (store, worker) = pulse_store::StateStore::open(root.path().join("state.db")).await?;
    let result = async {
        let snapshot = store.load_ssh_agent_identities(Arc::new(|| true)).await?;
        let (registry, documents, owner, page) = page()?;
        let (other_registry, _other_docs, _other_owner, other_page) = self::page()?;
        let bank = AgentBank::new();
        let generation = bank.generation.load(Ordering::Acquire);
        let receipt = bank.publish(
            &registry,
            page.clone(),
            snapshot.clone(),
            Vec::new(),
            generation,
        )?;
        let token = Uuid::parse_str(&receipt.token)?;
        assert!(bank.take(&other_registry, &other_page, token).is_err());
        assert!(bank.cancel(&registry, &other_page, token).is_err());
        let capture = bank.take(&registry, &page, token)?;
        assert!(capture.current(&registry));
        assert!(bank.take(&registry, &page, token).is_err());
        bank.cancel(&registry, &page, token)?;
        assert!(!capture.current(&registry));
        assert!(
            bank.publish(
                &registry,
                page.clone(),
                snapshot.clone(),
                Vec::new(),
                generation
            )
            .is_err()
        );
        bank.retire();
        let current = bank.generation.load(Ordering::Acquire);
        assert!(
            bank.publish(
                &registry,
                page.clone(),
                snapshot.clone(),
                Vec::new(),
                generation
            )
            .is_err()
        );
        assert!(
            bank.publish(
                &registry,
                page.clone(),
                snapshot.clone(),
                Vec::new(),
                current
            )
            .is_err()
        );
        drop(capture);
        let receipt = bank.publish(
            &registry,
            page.clone(),
            snapshot.clone(),
            Vec::new(),
            current,
        )?;
        let token = Uuid::parse_str(&receipt.token)?;
        let capture = bank.take(&registry, &page, token)?;
        documents.started(owner)?;
        assert!(!capture.current(&registry));
        assert!(bank.take(&registry, &page, token).is_err());
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(store);
    worker.join().await?;
    result
}
#[tokio::test]
async fn agent_metadata_cancelled_after_claim_rolls_back_before_directory_creation_and_cannot_replay()
-> TestResult {
    let root = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(root.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let (store, worker) = pulse_store::StateStore::open(root.path().join("state.db")).await?;
    let result = async {
        let snapshot = store.load_ssh_agent_identities(Arc::new(|| true)).await?;
        let (registry, _documents, _owner, page) = page()?;
        let bank = AgentBank::new();
        let generation = bank.generation.load(Ordering::Acquire);
        let fingerprint = format!("SHA256:{}", "A".repeat(43));
        let keys = vec![KeyDto {
            algorithm: "ssh-ed25519".to_owned(),
            fingerprint: fingerprint.clone(),
        }];
        let receipt = bank.publish(&registry, page.clone(), snapshot.clone(), keys, generation)?;
        let token = Uuid::parse_str(&receipt.token)?;
        let capture = bank.take(&registry, &page, token)?;
        let edit = AgentIdentityEdit::Create(SshAgentIdentity::new(
            "Selected",
            "agent-user",
            "ssh-ed25519",
            &fingerprint,
        )?);
        bank.cancel(&registry, &page, token)?;
        let guard = capture.clone();
        let authority: Arc<dyn Fn() -> bool + Send + Sync> =
            Arc::new(move || guard.current(&registry));
        assert!(
            store
                .edit_ssh_agent_identity(snapshot.clone(), edit, authority)
                .await
                .is_err()
        );
        assert!(store.load_ssh_agent_identities(Arc::new(|| true)).await? == snapshot);
        assert!(bank.take(&SessionRegistry::new(), &page, token).is_err());
        Ok::<(), Box<dyn std::error::Error + Send + Sync>>(())
    }
    .await;
    drop(store);
    worker.join().await?;
    result
}
