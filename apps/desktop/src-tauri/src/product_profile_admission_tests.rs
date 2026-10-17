use super::*;
use pulse_app::product_document::ProductDocuments;
use std::{future::Future, task::Poll};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Page {
    registry: SessionRegistry,
    documents: ProductDocuments,
    owner: pulse_app::registry::WindowToken,
    scope: Arc<PageLaunchScope>,
}
fn page() -> std::result::Result<Page, Box<dyn std::error::Error>> {
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window()?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let scope = documents.current(owner, nonce)?;
    Ok(Page {
        registry,
        documents,
        owner,
        scope,
    })
}
#[tokio::test]
async fn list_waits_actual_credit_single_waiter_retirement_never_releases_old_native_work()
-> TestResult {
    let p = page()?;
    let reader = ProfileReader::new();
    let old = reader.reserve()?;
    let mut queued = Box::pin(reader.reserve_list(&p.registry, &p.scope));
    assert!(std::future::poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx).is_pending())).await);
    assert_eq!(reader.read_waiter.available_permits(), 0);
    assert!(reader.reserve_list(&p.registry, &p.scope).await.is_err());
    p.documents.started(p.owner)?;
    reader.retire_read_waiters();
    assert!(queued.await.is_err());
    assert_eq!(reader.credit.available_permits(), 0);
    assert_eq!(reader.read_waiter.available_permits(), 1);
    drop(old);
    let nonce = p.documents.finished(p.owner)?;
    p.documents.confirm(p.owner, nonce)?;
    let next = p.documents.current(p.owner, nonce)?;
    let granted = reader.reserve_list(&p.registry, &next).await?;
    assert_eq!(reader.credit.available_permits(), 0);
    drop(granted);
    assert_eq!(reader.credit.available_permits(), 1);
    p.registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn dropped_list_waiter_or_reader_close_does_not_release_active_credit() -> TestResult {
    let p = page()?;
    let reader = ProfileReader::new();
    let old = reader.reserve()?;
    let mut queued = Box::pin(reader.reserve_list(&p.registry, &p.scope));
    assert!(std::future::poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx).is_pending())).await);
    drop(queued);
    assert_eq!(reader.read_waiter.available_permits(), 1);
    assert_eq!(reader.credit.available_permits(), 0);
    let mut queued = Box::pin(reader.reserve_list(&p.registry, &p.scope));
    assert!(std::future::poll_fn(|cx| Poll::Ready(queued.as_mut().poll(cx).is_pending())).await);
    reader.close();
    assert!(queued.await.is_err());
    assert_eq!(reader.credit.available_permits(), 0);
    drop(old);
    p.registry.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn original_five_second_read_budget_times_out_without_releasing_active_work() -> TestResult {
    let p = page()?;
    let reader = ProfileReader::new();
    let old = reader.reserve()?;
    let outcome = reader.reserve_list(&p.registry, &p.scope).await;
    assert!(matches!(outcome, Err("workspace.read_admission_timeout")));
    assert_eq!(reader.credit.available_permits(), 0);
    assert_eq!(reader.read_waiter.available_permits(), 1);
    drop(old);
    p.registry.shutdown()?;
    Ok(())
}
