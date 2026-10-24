//! Windows SQLite admission stays above platform/store and lives on the SQLite worker.
use pulse_platform::windows::{PrivateDatabase, PrivateDatabaseBackup, PrivateDirectory};
use pulse_store::{
    StoreError,
    database_admission::{DatabaseAdmission, DatabaseBackup},
};
use std::path::Path;
type Result<T> = std::result::Result<T, StoreError>;

struct Admission(PrivateDatabase);
struct Backup<'a>(PrivateDatabaseBackup<'a>);

impl DatabaseAdmission for Admission {
    fn validate(&self) -> Result<()> {
        self.0.validate().map_err(|_| StoreError::Location)
    }
    fn backup(&self, target_version: u32) -> Result<Box<dyn DatabaseBackup + '_>> {
        Ok(Box::new(Backup(
            self.0
                .backup(target_version)
                .map_err(|_| StoreError::Backup)?,
        )))
    }
}
impl DatabaseBackup for Backup<'_> {
    fn sync(&self) -> Result<()> {
        self.0.sync().map_err(|_| StoreError::Backup)
    }
}

/// Called inside the SQLite worker, never on the UI thread or from a page-selected path.
pub(super) fn admit(path: &Path) -> Result<Box<dyn DatabaseAdmission>> {
    if path.file_name().and_then(|name| name.to_str()) != Some("state.db") {
        return Err(StoreError::Location);
    }
    let parent = path.parent().ok_or(StoreError::Location)?;
    let directory = PrivateDirectory::open(parent)
        .inspect_err(|_| eprintln!("workspace.database_directory_admission_failed"))
        .map_err(|_| StoreError::Location)?;
    Ok(Box::new(Admission(
        directory
            .database()
            .inspect_err(|_| eprintln!("workspace.database_file_admission_failed"))
            .map_err(|_| StoreError::Location)?,
    )))
}

#[cfg(test)]
#[path = "database_windows_tests.rs"]
mod tests;
