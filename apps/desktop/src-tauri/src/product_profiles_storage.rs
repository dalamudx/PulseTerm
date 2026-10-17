//! Application-owned SQL worker and I/O admission, independent of any window reader.
use super::*;
pub(crate) struct ProfileStorage {
    pub(super) store: OnceCell<StoreState>,
    credit: Arc<Semaphore>,
    closed: AtomicBool,
    idle: watch::Sender<bool>,
    shutdown: OnceLock<watch::Receiver<Option<Result<()>>>>,
}
pub(super) struct StorageCredit {
    permit: Option<OwnedSemaphorePermit>,
    idle: watch::Sender<bool>,
}
impl Drop for StorageCredit {
    fn drop(&mut self) {
        drop(self.permit.take());
        self.idle.send_replace(true);
    }
}
impl ProfileStorage {
    pub(crate) fn new() -> Self {
        Self {
            store: OnceCell::new(),
            credit: Arc::new(Semaphore::new(1)),
            closed: AtomicBool::new(false),
            idle: watch::channel(true).0,
            shutdown: OnceLock::new(),
        }
    }
    pub(super) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }
    fn accept(&self, permit: OwnedSemaphorePermit) -> Result<StorageCredit> {
        let credit = StorageCredit {
            permit: Some(permit),
            idle: self.idle.clone(),
        };
        // Even a reservation that loses the close race must wake the physical drain.
        self.idle.send_replace(false);
        if self.is_closed() {
            return Err("workspace.denied");
        }
        Ok(credit)
    }
    pub(super) fn reserve(&self) -> Result<StorageCredit> {
        if self.is_closed() {
            return Err("workspace.denied");
        }
        let permit = self
            .credit
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        self.accept(permit)
    }
    pub(super) async fn reserve_wait(&self) -> Result<StorageCredit> {
        if self.is_closed() {
            return Err("workspace.denied");
        }
        let permit = self
            .credit
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "workspace.denied")?;
        self.accept(permit)
    }
    pub(super) fn cancel_store(&self) {
        if let Some(store) = self.store.get()
            && let Ok(task) = store.task.lock()
            && let Some(task) = task.as_ref()
        {
            task.cancel();
        }
    }
    fn close(&self) {
        self.closed.store(true, Ordering::SeqCst);
        self.credit.close();
        self.cancel_store();
    }
    /// A cancelled waiter cannot release application I/O capacity or lose the worker join.
    pub(crate) async fn shutdown(self: &Arc<Self>) -> Result<()> {
        self.close();
        let receipt = self.shutdown.get_or_init(|| {
            let (done, receipt) = watch::channel(None);
            let storage = self.clone();
            tokio::spawn(async move {
                let worker = tokio::spawn(async move { storage.physical().await });
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
    async fn physical(self: Arc<Self>) -> Result<()> {
        let mut idle = self.idle.subscribe();
        // No new reservation is possible after close. Notifications are only wakeups;
        // the actual permit count proves that the native holders have exited.
        while self.credit.available_permits() != 1 {
            idle.changed()
                .await
                .map_err(|_| "workspace.shutdown_failed")?;
        }
        if let Some(store) = self.store.get() {
            let task = store
                .task
                .lock()
                .map_err(|_| "workspace.shutdown_failed")?
                .take()
                .ok_or("workspace.shutdown_failed")?;
            task.shutdown()
                .await
                .map_err(|_| "workspace.shutdown_failed")?;
        }
        Ok(())
    }
}
impl Drop for ProfileStorage {
    fn drop(&mut self) {
        self.close();
    }
}
#[cfg(test)]
#[path = "product_profiles_storage_tests.rs"]
mod tests;
