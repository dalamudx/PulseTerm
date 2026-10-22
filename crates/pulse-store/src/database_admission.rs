//! Native database admission supplied by a trusted upper adapter; never page or credential authority.
use crate::Result;

/// Lives on the SQLite worker until the connection has actually closed.
/// Validation binds the exact database, private parent and existing sidecars.
/// Implementations must not log paths, account identifiers or database contents.
pub trait DatabaseAdmission: Send {
    /// Recheck file identity, owner, permissions and existing sidecars without changing them.
    fn validate(&self) -> Result<()>;
    /// Exclusively create and pin the migration backup before SQLite opens it.
    fn backup(&self, target_version: u32) -> Result<Box<dyn DatabaseBackup + '_>>;
}

/// Pins a new backup through SQLite close and the final file-data flush.
pub trait DatabaseBackup {
    /// Flush and revalidate after the SQLite backup destination has closed.
    fn sync(&self) -> Result<()>;
}

#[cfg(test)]
#[path = "database_admission_tests.rs"]
mod tests;
