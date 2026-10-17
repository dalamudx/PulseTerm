//! Device-local recent successful saved SSH targets. No credentials, raw addresses or use authority.
use crate::{Request, Result, StateStore, StoreError};
use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::oneshot;
use uuid::Uuid;
const KEY: &str = "catalogue.recent-ssh-hosts.v1";
const LIMIT: usize = 32;
const BYTES: usize = 8192;
/// One saved target whose protocol session was successfully registered.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct RecentHost {
    /// Backend-generated saved host identity; never a network target.
    pub host: String,
    /// Backend time of the latest successful registration, in Unix milliseconds.
    pub connected_at: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    version: u8,
    entries: Vec<RecentHost>,
}
pub(crate) struct RecentRequest {
    owner: Uuid,
    host: Option<Uuid>,
    authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    reply: oneshot::Sender<Result<Vec<RecentHost>>>,
}
impl StateStore {
    /// Read bounded local metadata with the caller's credit held to actual SQL exit.
    pub async fn load_recent_ssh_hosts(
        &self,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Vec<RecentHost>> {
        self.recent_hosts(None, authorize).await
    }
    /// Trusted backend calls this only after successful protocol-session adoption.
    /// There is intentionally no page command that can append recent targets.
    pub async fn record_recent_ssh_host(&self, host: Uuid) -> Result<()> {
        self.recent_hosts(Some(host), Arc::new(|| true))
            .await
            .map(|_| ())
    }
    async fn recent_hosts(
        &self,
        host: Option<Uuid>,
        authorize: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Result<Vec<RecentHost>> {
        let (reply, result) = oneshot::channel();
        self.admit(Request::RecentHosts(Box::new(RecentRequest {
            owner: self.host_owner,
            host,
            authorize,
            reply,
        })))?;
        result.await.map_err(|_| StoreError::Closed)?
    }
}
fn allowed(gate: &dyn Fn() -> bool) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(gate)).unwrap_or(false)
}
fn load(connection: &Connection) -> Result<Document> {
    let row: Option<(i64,Vec<u8>)> = connection.query_row("SELECT length(CAST(value AS BLOB)),substr(CAST(value AS BLOB),1,8193) FROM app_state WHERE key=?1",[KEY],|row|Ok((row.get(0)?,row.get(1)?))).optional().map_err(|_|StoreError::Database)?;
    let Some((length, bytes)) = row else {
        return Ok(Document {
            version: 1,
            entries: vec![],
        });
    };
    if !(0..=BYTES as i64).contains(&length) {
        return Err(StoreError::HostRecord);
    }
    let doc: Document = serde_json::from_slice(&bytes).map_err(|_| StoreError::HostRecord)?;
    if doc.version != 1 || doc.entries.len() > LIMIT {
        return Err(StoreError::HostRecord);
    }
    let mut unique = std::collections::HashSet::new();
    for row in &doc.entries {
        let host = Uuid::parse_str(&row.host).map_err(|_| StoreError::HostRecord)?;
        if host.to_string() != row.host
            || host.get_version_num() != 7
            || host.get_variant() != uuid::Variant::RFC4122
            || !unique.insert(host)
            || row.connected_at == 0
            || row.connected_at > i64::MAX as u64
        {
            return Err(StoreError::HostRecord);
        }
    }
    Ok(doc)
}
fn operation(
    connection: &Connection,
    owner: Uuid,
    host: Option<Uuid>,
    gate: &dyn Fn() -> bool,
) -> Result<Vec<RecentHost>> {
    if !allowed(gate) {
        return Err(StoreError::HostAuthorization);
    }
    let tx = connection
        .unchecked_transaction()
        .map_err(|_| StoreError::Database)?;
    let active = crate::ssh_hosts::load(&tx, owner)?;
    let mut doc = load(&tx)?;
    doc.entries.retain(|row| {
        active
            .records()
            .iter()
            .any(|h| h.id().to_string() == row.host)
    });
    if let Some(host) = host {
        if !active.records().iter().any(|h| h.id() == host) {
            return Err(StoreError::HostConflict);
        }
        let time: u64 = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| StoreError::HostRecord)?
            .as_millis()
            .try_into()
            .map_err(|_| StoreError::HostRecord)?;
        if time == 0 || time > i64::MAX as u64 {
            return Err(StoreError::HostRecord);
        }
        doc.entries.retain(|row| row.host != host.to_string());
        doc.entries.insert(
            0,
            RecentHost {
                host: host.to_string(),
                connected_at: time,
            },
        );
        doc.entries.truncate(LIMIT);
        let data = serde_json::to_string(&doc).map_err(|_| StoreError::HostRecord)?;
        if data.len() > BYTES {
            return Err(StoreError::HostRecord);
        }
        tx.execute("INSERT INTO app_state(key,value) VALUES(?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value",rusqlite::params![KEY,data]).map_err(|_|StoreError::Database)?;
    }
    if !allowed(gate) {
        return Err(StoreError::HostAuthorization);
    }
    tx.commit().map_err(|_| StoreError::Database)?;
    Ok(doc.entries)
}
pub(crate) fn serve(connection: &Connection, request: RecentRequest) {
    let RecentRequest {
        owner,
        host,
        authorize,
        reply,
    } = request;
    // Successful backend registration writes survive a dropped receipt; reads may skip.
    let result = if host.is_some() || !reply.is_closed() {
        operation(connection, owner, host, &*authorize)
    } else {
        Err(StoreError::Closed)
    };
    drop(authorize);
    let _ = reply.send(result);
}
#[cfg(test)]
#[path = "recent_hosts_tests.rs"]
mod tests;
