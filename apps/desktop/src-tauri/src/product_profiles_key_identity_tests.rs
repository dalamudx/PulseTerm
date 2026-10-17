use super::*;
use pulse_app::product_document::ProductDocuments;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
async fn queued_source_reclamation(close: bool) -> TestResult {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    let root = tempfile::tempdir()?;
    let dir = root.path().join("state");
    let registry = SessionRegistry::new();
    let documents = ProductDocuments::new(registry.clone());
    let owner = registry.open_window().map_err(|_| "fixture.window")?;
    documents.install(owner)?;
    documents.started(owner)?;
    let nonce = documents.finished(owner)?;
    documents.confirm(owner, nonce)?;
    let page = documents.current(owner, nonce)?;
    let reader = Arc::new(ProfileReader::new());
    let store = reader
        .prepare_store(&registry, &page, dir.clone(), reader.reserve()?)
        .await?;
    let (entered, started) = tokio::sync::oneshot::channel();
    let entered = Mutex::new(Some(entered));
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let first = AtomicBool::new(true);
    let gate = Arc::new(move || {
        if first.swap(false, Ordering::SeqCst) {
            if let Ok(mut entered) = entered.lock()
                && let Some(entered) = entered.take()
            {
                let _ = entered.send(());
            }
            return released
                .lock()
                .is_ok_and(|released| released.recv_timeout(Duration::from_secs(5)).is_ok());
        }
        true
    });
    let blocker = tokio::spawn(async move { store.load_ssh_key_identities(gate).await });
    let result: TestResult = async {
        tokio::time::timeout(Duration::from_secs(3), started).await??;
        let mut pending = Box::pin(reader.key_identities(registry.clone(), page, dir));
        std::future::poll_fn(|cx| {
            use std::future::Future;
            match pending.as_mut().poll(cx) {
                std::task::Poll::Pending => std::task::Poll::Ready(Ok(())),
                std::task::Poll::Ready(_) => std::task::Poll::Ready(Err("fixture.must_queue")),
            }
        })
        .await?;
        // The prepared cache has no intervening wait; this drives the inner task to the same worker queue.
        tokio::task::yield_now().await;
        if reader.credit.available_permits() != 0 {
            return Err("fixture.credit_not_held".into());
        }
        drop(pending);
        if close {
            reader.close();
        } else {
            documents.started(owner)?;
        }
        tokio::task::yield_now().await;
        if reader.credit.available_permits() != 0 {
            return Err("fixture.cancel_released_active_credit".into());
        }
        Ok(())
    }
    .await;
    // Always release the task-owned native predicate before dropping its directory, including probe errors.
    let _ = release.send(());
    let blocked = tokio::time::timeout(Duration::from_secs(5), blocker).await;
    let drained = tokio::time::timeout(Duration::from_secs(5), reader.shutdown()).await;
    registry.shutdown()?;
    result?;
    blocked???;
    drained??;
    if reader.credit.available_permits() != 1 {
        return Err("fixture.native_credit_not_returned".into());
    }
    Ok(())
}
#[tokio::test]
async fn key_source_cancelled_waiter_keeps_credit_until_queued_sql_exit() -> TestResult {
    queued_source_reclamation(false).await
}
#[tokio::test]
async fn key_source_owner_close_drains_cancelled_queued_sql_before_returning_credit() -> TestResult
{
    queued_source_reclamation(true).await
}
