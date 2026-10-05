//! Bounded local app-state storage. No credential or arbitrary SQL interface.
mod database;
/// Automatic Unix profile visibility and deletion preferences; no executable paths.
pub mod profiles;
/// Pure bounded terminal setting overlays; no file I/O or permissions.
pub mod settings;
pub mod settings_edit;
/// Internal credential AEAD primitive, not a keyring, persistence or plaintext IPC.
pub mod vault_crypto;
/// Fixed-cost memory-only master-password validation primitive, not unlock IPC.
pub mod vault_kdf;
use profiles::{AutoProfileEdit, AutoProfilePreferences};
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
use tokio_util::sync::CancellationToken;
/// Maximum requests waiting for the single database worker.
pub const REQUEST_LIMIT: usize = 16;
/// Stable failures never include database paths, SQL, or record contents.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum StoreError {
    /// The caller did not provide a private regular database location.
    #[error("store.private_location_required")]
    Location,
    /// SQLite or filesystem operation failed; no sensitive diagnostic is exposed.
    #[error("store.database_failed")]
    Database,
    /// Migration version is unknown or legacy unversioned tables are present.
    #[error("store.unsupported_schema")]
    Schema,
    /// Existing backup is preserved; migration must not silently overwrite it.
    #[error("store.backup_failed")]
    Backup,
    /// Malformed, oversized, incompatible, duplicated, or conflicting preferences.
    #[error("store.invalid_profile_preferences")]
    Record,
    /// The current preferences differ from the snapshot expected by a replacement.
    #[error("store.profile_preferences_conflict")]
    Conflict,
    /// No new request was admitted to the bounded worker queue.
    #[error("store.busy")]
    Busy,
    /// Database worker has ended; no write retry is implied.
    #[error("store.closed")]
    Closed,
}
type Result<T> = std::result::Result<T, StoreError>;
enum Request {
    Load(oneshot::Sender<Result<AutoProfilePreferences>>),
    Save(
        AutoProfilePreferences,
        AutoProfilePreferences,
        oneshot::Sender<Result<()>>,
    ),
    Edit(
        AutoProfileEdit,
        oneshot::Sender<Result<AutoProfilePreferences>>,
    ),
}
/// Backend-only typed handle. All reads and writes run on one blocking worker.
/// No database access, application directory creation, or native work on UI threads.
#[derive(Clone)]
pub struct StateStore {
    requests: mpsc::Sender<Request>,
    cancellation: CancellationToken,
}
/// Trusted worker owner. Drop or explicit cancellation closes the worker even if
/// clients retain handles; already executing database writes cannot be recalled.
pub struct StateStoreTask {
    task: Option<tokio::task::JoinHandle<Result<()>>>,
    cancellation: CancellationToken,
}
impl StateStore {
    /// Open a database in a caller-provisioned trusted private directory.
    /// Unix requires private directory/file modes and no database symlink; Windows
    /// ACL provisioning is the trusted platform adapter's responsibility.
    pub async fn open(path: PathBuf) -> Result<(Self, StateStoreTask)> {
        let (requests, mut inbox) = mpsc::channel(REQUEST_LIMIT);
        let (ready, initialized) = oneshot::channel();
        let cancellation = CancellationToken::new();
        let worker_cancellation = cancellation.clone();
        let runtime = tokio::runtime::Handle::current();
        let task = tokio::task::spawn_blocking(move || {
            let connection = match database::open(&path) {
                Ok(c) => c,
                Err(e) => {
                    let _ = ready.send(Err(e));
                    return Err(e);
                }
            };
            if ready.send(Ok(())).is_err() {
                return Ok(());
            }
            serve(connection, &mut inbox, &worker_cancellation, &runtime)
        });
        match initialized.await.map_err(|_| StoreError::Closed)? {
            Ok(()) => Ok((
                Self {
                    requests,
                    cancellation: cancellation.clone(),
                },
                StateStoreTask {
                    task: Some(task),
                    cancellation,
                },
            )),
            Err(e) => {
                drop(requests);
                let _ = task.await;
                Err(e)
            }
        }
    }
    fn admit(&self, request: Request) -> Result<()> {
        if self.cancellation.is_cancelled() {
            return Err(StoreError::Closed);
        }
        self.requests.try_send(request).map_err(|e| match e {
            mpsc::error::TrySendError::Full(_) => StoreError::Busy,
            mpsc::error::TrySendError::Closed(_) => StoreError::Closed,
        })
    }
    /// Load validated preferences. Malformed data never becomes an empty default.
    pub async fn load_auto_profiles(&self) -> Result<AutoProfilePreferences> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Load(reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Compare expected preferences and atomically replace the row, or return Conflict.
    /// No unconditional snapshot write is available. Dropping the caller
    /// before worker execution cancels it; an already executing write cannot be
    /// recalled. A lost response does not justify an automatic retry.
    pub async fn save_auto_profiles(
        &self,
        expected: AutoProfilePreferences,
        value: AutoProfilePreferences,
    ) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Save(expected, value, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
    /// Apply one typed edit to current durable preferences in an IMMEDIATE transaction.
    /// The result is this edit's committed snapshot, not a guarantee that a later edit
    /// has not already committed. No catalog or session is changed by this API.
    /// Cancel before execution suppresses the edit; already executing writes cannot
    /// be recalled and a lost response must not be automatically retried.
    pub async fn edit_auto_profiles(
        &self,
        edit: AutoProfileEdit,
    ) -> Result<AutoProfilePreferences> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Edit(edit, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
impl StateStoreTask {
    /// Revoke future admission and wake the worker without consuming queue credit.
    /// Pending requests are closed, not replayed; an executing write may still commit.
    pub fn cancel(&self) {
        self.cancellation.cancel();
    }
    /// Cancel and wait for the actual worker/database closure, retaining no quota.
    pub async fn shutdown(self) -> Result<()> {
        self.cancel();
        self.join().await
    }
    /// Wait for orderly closure. Drop all client handles or first request cancellation.
    /// Cancelling this wait drops the owner and requests shutdown, not a write rollback.
    pub async fn join(mut self) -> Result<()> {
        self.task
            .take()
            .ok_or(StoreError::Closed)?
            .await
            .map_err(|_| StoreError::Closed)?
    }
}
impl Drop for StateStoreTask {
    fn drop(&mut self) {
        self.cancellation.cancel();
    }
}

fn serve(
    connection: rusqlite::Connection,
    inbox: &mut mpsc::Receiver<Request>,
    cancellation: &CancellationToken,
    runtime: &tokio::runtime::Handle,
) -> Result<()> {
    loop {
        let request=runtime.block_on(async {
            tokio::select! { biased; _=cancellation.cancelled()=>None, request=inbox.recv()=>request }
        });
        let Some(request) = request else {
            break;
        };
        if cancellation.is_cancelled() {
            break;
        }
        dispatch(&connection, request);
    }
    connection.close().map_err(|_| StoreError::Database)
}

fn dispatch(connection: &rusqlite::Connection, request: Request) {
    match request {
        Request::Load(reply) if !reply.is_closed() => {
            let _ = reply.send(database::load(connection));
        }
        Request::Save(expected, value, reply) if !reply.is_closed() => {
            let _ = reply.send(database::save(connection, &expected, &value));
        }
        Request::Edit(edit, reply) if !reply.is_closed() => {
            let _ = reply.send(database::edit(connection, edit));
        }
        _ => {}
    }
}
#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod edit_tests;
#[cfg(all(test, unix))]
mod privacy_tests;
#[cfg(test)]
mod tests;
