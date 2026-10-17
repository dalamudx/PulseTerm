//! Public trust rows retain future sync metadata, but do not implement synchronization or authority.
use super::*;
use pulse_core::hlc::Hlc;
#[cfg(all(test, feature = "ssh-trust"))]
#[path = "ssh_trust_migration_tests.rs"]
mod tests;
const MAX_DELETED: usize = 256;
#[derive(Clone, PartialEq, Eq)]
pub(crate) struct StoredPin {
    pub(crate) id: Uuid,
    pub(crate) created_at: u64,
    pub(crate) stamp: Hlc,
    pub(crate) deleted: bool,
    pub(crate) target: TrustTarget,
    pub(crate) key: Option<String>,
}
pub(crate) const SCHEMA: &str = "
CREATE TABLE known_hosts_state(singleton INTEGER PRIMARY KEY CHECK(singleton=1),schema_version INTEGER NOT NULL CHECK(schema_version=1),revision INTEGER NOT NULL CHECK(revision>=0),node_id TEXT,clock TEXT) STRICT;
INSERT INTO known_hosts_state VALUES(1,1,0,NULL,NULL);
CREATE TABLE known_hosts(id TEXT PRIMARY KEY CHECK(length(id)=36),created_at INTEGER NOT NULL CHECK(created_at>0),hlc TEXT NOT NULL CHECK(length(hlc) BETWEEN 1 AND 67),deleted INTEGER NOT NULL DEFAULT 0 CHECK(deleted IN (0,1)),host TEXT NOT NULL,port INTEGER NOT NULL CHECK(port BETWEEN 1 AND 65535),key TEXT,ordinal INTEGER NOT NULL UNIQUE CHECK(ordinal>=0),UNIQUE(host,port),CHECK((deleted=0 AND key IS NOT NULL) OR (deleted=1 AND key IS NULL))) STRICT;";
fn uuid(text: &str) -> Result<Uuid> {
    let id = Uuid::parse_str(text).map_err(|_| StoreError::HostTrustRecord)?;
    if id.get_version_num() != 7 || id.to_string() != text {
        return Err(StoreError::HostTrustRecord);
    }
    Ok(id)
}
pub(crate) fn import_legacy(tx: &Transaction<'_>) -> Result<()> {
    let prior = super::load_legacy(tx, Uuid::nil())?;
    save(tx, &prior)?;
    tx.execute("DELETE FROM app_state WHERE key=?1", [KEY])
        .map_err(|_| StoreError::Database)?;
    let _checked = load(tx, Uuid::nil())?;
    Ok(())
}
pub(crate) fn load(c: &Connection, owner: Uuid) -> Result<TrustSnapshot> {
    let legacy: bool = c
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM app_state WHERE key=?1)",
            [KEY],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::Database)?;
    if legacy {
        return Err(StoreError::HostTrustRecord);
    }
    let count: i64 = c
        .query_row(
            "SELECT count(*) FROM (SELECT 1 FROM known_hosts_state LIMIT 2)",
            [],
            |r| r.get(0),
        )
        .map_err(|_| StoreError::HostTrustRecord)?;
    if count != 1 {
        return Err(StoreError::HostTrustRecord);
    }
    let (slot,format,revision,node,clock):(i64,i64,i64,Option<String>,Option<String>)=c.query_row("SELECT singleton,schema_version,revision,CASE WHEN length(node_id)<=36 THEN node_id ELSE NULL END,CASE WHEN length(clock)<=67 THEN clock ELSE NULL END FROM known_hosts_state",[],|r|Ok((r.get(0)?,r.get(1)?,r.get(2)?,r.get(3)?,r.get(4)?))).map_err(|_|StoreError::HostTrustRecord)?;
    if slot != 1 || format != 1 || revision < 0 {
        return Err(StoreError::HostTrustRecord);
    }
    let clock = match (revision, node, clock) {
        (0, None, None) => None,
        (1.., Some(node), Some(clock)) => {
            let stamp = Hlc::parse(&clock).map_err(|_| StoreError::HostTrustRecord)?;
            if stamp.node() != uuid(&node)? {
                return Err(StoreError::HostTrustRecord);
            }
            Some(stamp)
        }
        _ => return Err(StoreError::HostTrustRecord),
    };
    let mut statement=c.prepare("SELECT length(CAST(host AS BLOB)),length(CAST(key AS BLOB)),CASE WHEN length(CAST(host AS BLOB))<=253 THEN host ELSE NULL END,port,CASE WHEN length(CAST(key AS BLOB))<=8192 THEN key ELSE NULL END,ordinal,CASE WHEN length(id)=36 THEN id ELSE NULL END,created_at,CASE WHEN length(hlc)<=67 THEN hlc ELSE NULL END,deleted FROM known_hosts ORDER BY ordinal LIMIT 385").map_err(|_|StoreError::Database)?;
    let mut values = statement.query([]).map_err(|_| StoreError::Database)?;
    let mut rows = Vec::new();
    let mut records = Vec::new();
    let mut raw_bytes = 0usize;
    let mut deleted_count = 0;
    while let Some(r) = values.next().map_err(|_| StoreError::Database)? {
        if rows.len() >= MAX_HOSTS + MAX_DELETED {
            return Err(StoreError::HostTrustRecord);
        }
        let hlen: i64 = r.get(0).map_err(|_| StoreError::HostTrustRecord)?;
        let klen: Option<i64> = r.get(1).map_err(|_| StoreError::HostTrustRecord)?;
        let deleted: i64 = r.get(9).map_err(|_| StoreError::HostTrustRecord)?;
        if !(1..=253).contains(&hlen)
            || !matches!(deleted, 0 | 1)
            || match (deleted, klen) {
                (0, Some(n)) => !(5..=8192).contains(&n),
                (1, None) => false,
                _ => true,
            }
        {
            return Err(StoreError::HostTrustRecord);
        }
        raw_bytes = raw_bytes
            .checked_add((hlen + klen.unwrap_or(0)) as usize)
            .filter(|n| *n <= LIMIT)
            .ok_or(StoreError::HostTrustRecord)?;
        let host: String = r.get(2).map_err(|_| StoreError::HostTrustRecord)?;
        let port: u16 = r.get(3).map_err(|_| StoreError::HostTrustRecord)?;
        let key: Option<String> = r.get(4).map_err(|_| StoreError::HostTrustRecord)?;
        let ordinal: i64 = r.get(5).map_err(|_| StoreError::HostTrustRecord)?;
        let id = uuid(
            &r.get::<_, String>(6)
                .map_err(|_| StoreError::HostTrustRecord)?,
        )?;
        let created_at = u64::try_from(
            r.get::<_, i64>(7)
                .map_err(|_| StoreError::HostTrustRecord)?,
        )
        .map_err(|_| StoreError::HostTrustRecord)?;
        let stamp = Hlc::parse(
            &r.get::<_, String>(8)
                .map_err(|_| StoreError::HostTrustRecord)?,
        )
        .map_err(|_| StoreError::HostTrustRecord)?;
        let catalog = clock.ok_or(StoreError::HostTrustRecord)?;
        let target = TrustTarget::new(&host, port)?;
        if target.host() != host
            || ordinal != rows.len() as i64
            || created_at == 0
            || created_at > stamp.wall()
            || stamp > catalog
            || stamp.node() != catalog.node()
            || rows
                .iter()
                .any(|p: &StoredPin| p.id == id || p.target == target)
        {
            return Err(StoreError::HostTrustRecord);
        }
        if deleted == 0 {
            if records.len() >= MAX_HOSTS || deleted_count != 0 {
                return Err(StoreError::HostTrustRecord);
            }
            records.push(TrustRecord::new(
                target.clone(),
                key.clone().ok_or(StoreError::HostTrustRecord)?,
            )?);
        } else {
            deleted_count += 1;
            if deleted_count > MAX_DELETED {
                return Err(StoreError::HostTrustRecord);
            }
        }
        rows.push(StoredPin {
            id,
            created_at,
            stamp,
            deleted: deleted == 1,
            target,
            key,
        });
    }
    let snapshot = TrustSnapshot {
        owner,
        revision: revision as u64,
        records,
        rows,
        clock,
    };
    if revision == 0 {
        if !snapshot.rows.is_empty() {
            return Err(StoreError::HostTrustRecord);
        }
    } else {
        let _budget = encode(&snapshot)?;
    }
    Ok(snapshot)
}
pub(crate) fn save(tx: &Transaction<'_>, snapshot: &TrustSnapshot) -> Result<()> {
    if snapshot.revision == 0 {
        if !snapshot.records.is_empty() || !snapshot.rows.is_empty() {
            return Err(StoreError::HostTrustRecord);
        }
        return Ok(());
    }
    let _budget = encode(snapshot)?;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|_| StoreError::HostTrustRevision)?
        .as_millis();
    let now = u64::try_from(now).map_err(|_| StoreError::HostTrustRevision)?;
    let stamp = match snapshot.clock {
        Some(h) => h.next(now),
        None => Hlc::new(now, 0, Uuid::now_v7()),
    }
    .map_err(|_| StoreError::HostTrustRevision)?;
    let mut rows = Vec::new();
    for record in snapshot.records() {
        let prior = snapshot.rows.iter().find(|p| p.target == record.target);
        let changed = prior.is_none_or(|p| p.deleted || p.key.as_deref() != Some(record.key()));
        rows.push(StoredPin {
            id: prior.map_or_else(Uuid::now_v7, |p| p.id),
            created_at: prior.map_or(stamp.wall(), |p| p.created_at),
            stamp: if changed {
                stamp
            } else {
                prior.ok_or(StoreError::HostTrustRecord)?.stamp
            },
            deleted: false,
            target: record.target.clone(),
            key: Some(record.key.clone()),
        });
    }
    for prior in &snapshot.rows {
        if snapshot.records.iter().all(|r| r.target() != &prior.target) {
            let mut row = prior.clone();
            if !row.deleted {
                row.deleted = true;
                row.key = None;
                row.stamp = stamp;
            }
            rows.push(row);
        }
    }
    if rows.len() - snapshot.records.len() > MAX_DELETED {
        return Err(StoreError::HostTrustRecord);
    }
    let raw: usize = rows
        .iter()
        .map(|r| r.target.host().len() + r.key.as_ref().map_or(0, String::len))
        .sum();
    if raw > LIMIT {
        return Err(StoreError::HostTrustRecord);
    }
    tx.execute("DELETE FROM known_hosts", [])
        .map_err(|_| StoreError::Database)?;
    for (ordinal, row) in rows.iter().enumerate() {
        tx.execute("INSERT INTO known_hosts(id,created_at,hlc,deleted,host,port,key,ordinal) VALUES (?1,?2,?3,?4,?5,?6,?7,?8)",rusqlite::params![row.id.to_string(),row.created_at as i64,row.stamp.encode(),i64::from(row.deleted),row.target.host(),row.target.port(),row.key,ordinal as i64]).map_err(|_|StoreError::Database)?;
    }
    if tx.execute("UPDATE known_hosts_state SET revision=?1,node_id=?2,clock=?3 WHERE singleton=1 AND schema_version=1",rusqlite::params![snapshot.revision() as i64,stamp.node().to_string(),stamp.encode()]).map_err(|_|StoreError::Database)?!=1 {return Err(StoreError::HostTrustRecord);}
    Ok(())
}
