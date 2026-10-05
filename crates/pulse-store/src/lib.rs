//! Bounded local app-state storage. No credential or arbitrary SQL interface.
mod database;
/// Automatic Unix profile visibility and deletion preferences; no executable paths.
pub mod profiles;
use profiles::AutoProfilePreferences;
use std::path::PathBuf;
use tokio::sync::{mpsc, oneshot};
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
    Save(AutoProfilePreferences, oneshot::Sender<Result<()>>),
}
/// Backend-only typed handle. All reads and writes run on one blocking worker.
/// No database access, application directory creation, or native work on UI threads.
#[derive(Clone)]
pub struct StateStore {
    requests: mpsc::Sender<Request>,
}
/// Join receipt for worker shutdown after every handle is dropped.
pub struct StateStoreTask {
    task: tokio::task::JoinHandle<Result<()>>,
}
impl StateStore {
    /// Open a database in a caller-provisioned trusted private directory.
    /// Unix requires private directory/file modes and no database symlink; Windows
    /// ACL provisioning is the trusted platform adapter's responsibility.
    pub async fn open(path: PathBuf) -> Result<(Self, StateStoreTask)> {
        let (requests, mut inbox) = mpsc::channel(REQUEST_LIMIT);
        let (ready, initialized) = oneshot::channel();
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
            while let Some(request) = inbox.blocking_recv() {
                dispatch(&connection, request);
            }
            connection.close().map_err(|_| StoreError::Database)
        });
        match initialized.await.map_err(|_| StoreError::Closed)? {
            Ok(()) => Ok((Self { requests }, StateStoreTask { task })),
            Err(e) => {
                drop(requests);
                let _ = task.await;
                Err(e)
            }
        }
    }
    fn admit(&self, request: Request) -> Result<()> {
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
    /// Atomically replace the single typed preferences row. Dropping the caller
    /// before worker execution cancels it; an already executing write cannot be
    /// recalled. A lost response does not justify an automatic retry.
    pub async fn save_auto_profiles(&self, value: AutoProfilePreferences) -> Result<()> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::Save(value, reply))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
impl StateStoreTask {
    /// Wait for orderly closure. Every cloned StateStore must first be dropped.
    pub async fn join(self) -> Result<()> {
        self.task.await.map_err(|_| StoreError::Closed)?
    }
}
fn dispatch(connection: &rusqlite::Connection, request: Request) {
    match request {
        Request::Load(reply) if !reply.is_closed() => {
            let _ = reply.send(database::load(connection));
        }
        Request::Save(value, reply) if !reply.is_closed() => {
            let _ = reply.send(database::save(connection, &value));
        }
        _ => {}
    }
}
#[cfg(test)]
mod cancellation_tests;
#[cfg(test)]
mod tests;
