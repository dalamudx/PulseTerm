use super::*;
use pulse_app::product_document::ProductDocuments;
use pulse_core::ssh_endpoint::SshEndpoint;
use pulse_store::ssh_hosts::HostEdit;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
fn page() -> Result<(
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
)> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().map_err(|_| "fixture.window")?;
    documents.install(owner).map_err(|_| "fixture.install")?;
    documents.started(owner).map_err(|_| "fixture.started")?;
    let nonce = documents.finished(owner).map_err(|_| "fixture.finished")?;
    documents
        .confirm(owner, nonce)
        .map_err(|_| "fixture.confirm")?;
    let scope = documents
        .current(owner, nonce)
        .map_err(|_| "fixture.scope")?;
    Ok((registry, documents, owner, scope))
}
#[tokio::test]
async fn saved_metadata_reads_without_shell_discovery_and_rereads_shared_source() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
    let (store, worker) = StateStore::open(dir.join("state.db")).await?;
    let empty = store.load_ssh_hosts().await?;
    let saved = store
        .edit_ssh_hosts(
            empty,
            HostEdit::Create(SshEndpoint::new("开发机", "Example.COM", 2222, "user")?),
            Arc::new(|| true),
        )
        .await?;
    let id = saved.records()[0].id();
    let (registry, _documents, _owner, scope) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let loaded = reader
        .hosts(registry.clone(), scope.clone(), dir.clone())
        .await?;
    assert_eq!(loaded.revision(), 1);
    assert_eq!(loaded.records()[0].id(), id);
    assert_eq!(
        loaded.records()[0].endpoint().target().host(),
        "example.com"
    );
    store
        .edit_ssh_hosts(
            saved,
            HostEdit::Update(id, SshEndpoint::new("新名字", "example.com", 22, "user")?),
            Arc::new(|| true),
        )
        .await?;
    let next = reader.hosts(registry.clone(), scope, dir).await?;
    assert_eq!(next.revision(), 2);
    assert_eq!(next.records()[0].endpoint().label(), "新名字");
    reader.shutdown().await?;
    assert_eq!(store.load_ssh_hosts().await?.revision(), 2);
    drop(store);
    worker.join().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn retired_queued_page_creates_no_directory_and_new_page_reads_empty_current_source()
-> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("state");
    let (registry, documents, owner, scope) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let pending = reader.hosts(registry.clone(), scope, dir.clone());
    tokio::pin!(pending);
    std::future::poll_fn(|cx| {
        use std::future::Future;
        match pending.as_mut().poll(cx) {
            std::task::Poll::Pending => std::task::Poll::Ready(()),
            std::task::Poll::Ready(_) => panic!("supervisor must be queued"),
        }
    })
    .await;
    documents.started(owner)?;
    assert!(matches!(pending.await, Err("workspace.denied")));
    assert!(!dir.exists());
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let current = documents.current(owner, nonce)?;
    let snapshot = reader.hosts(registry.clone(), current, dir).await?;
    assert_eq!(snapshot.revision(), 0);
    assert!(snapshot.records().is_empty());
    reader.shutdown().await?;
    registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn foreign_scope_directory_substitution_and_closed_reader_never_publish() -> TestResult {
    let root = tempfile::tempdir()?;
    let dir = root.path().join("state");
    let (registry, _documents, _owner, scope) = page()?;
    let (foreign, _foreign_docs, _foreign_owner, _foreign_scope) = page()?;
    let reader = Arc::new(ProfileReader::new());
    assert!(matches!(
        reader
            .hosts(foreign.clone(), scope.clone(), dir.clone())
            .await,
        Err("workspace.denied")
    ));
    assert!(!dir.exists());
    reader.hosts(registry.clone(), scope.clone(), dir).await?;
    let other = root.path().join("other");
    assert!(matches!(
        reader
            .hosts(registry.clone(), scope.clone(), other.clone())
            .await,
        Err("workspace.denied")
    ));
    assert!(!other.exists());
    reader.shutdown().await?;
    assert!(matches!(
        reader.hosts(registry.clone(), scope, other).await,
        Err("workspace.denied")
    ));
    registry.shutdown()?;
    foreign.shutdown()?;
    Ok(())
}
