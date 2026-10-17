//! A cancelled window-drain waiter cannot detach the physical drain or lose its result.
use super::*;

pub(super) async fn drain(reader: &Arc<ProfileReader>) -> Result<()> {
    reader.close();
    let receipt = reader.shutdown.get_or_init(|| {
        let (done, receipt) = watch::channel(None);
        let reader = reader.clone();
        tokio::spawn(async move {
            let worker = tokio::spawn(async move { physical(reader).await });
            let result = worker.await.unwrap_or(Err("workspace.shutdown_failed"));
            done.send_replace(Some(result));
        });
        receipt
    });
    let mut receipt = receipt.clone();
    loop {
        if let Some(result) = *receipt.borrow_and_update() {
            return result;
        }
        receipt
            .changed()
            .await
            .map_err(|_| "workspace.shutdown_failed")?;
    }
}
async fn physical(reader: Arc<ProfileReader>) -> Result<()> {
    let mut idle = reader.idle.subscribe();
    while reader.credit.available_permits() != 1 {
        idle.changed()
            .await
            .map_err(|_| "workspace.shutdown_failed")?;
    }
    // A window's completed I/O is not permission to cancel or join the shared worker.
    Ok(())
}
#[cfg(test)]
#[path = "product_profiles_shutdown_tests.rs"]
mod tests;
