//! Windows settings adapter; production uses the original page-bound SettingsOwner supervision.
//! Platform handles live above store; application page/Vault authority stays with the caller.
use pulse_store::{
    settings::{SETTINGS_TEXT_LIMIT, SettingsError, SettingsPatch},
    settings_edit::set_terminal_setting,
    settings_file::{SettingsFileError as Error, SettingsMutation},
    settings_remove::remove_terminal_setting,
};
type Result<T> = std::result::Result<T, Error>;

fn document(bytes: &[u8]) -> Result<&str> {
    if bytes.len() > SETTINGS_TEXT_LIMIT {
        return Err(SettingsError::Size.into());
    }
    let text = std::str::from_utf8(bytes).map_err(|_| SettingsError::Document)?;
    SettingsPatch::parse(text)?;
    Ok(text)
}
fn edit(original: &str, mutation: SettingsMutation) -> Result<String> {
    document(original.as_bytes())?;
    let changed = match mutation {
        SettingsMutation::Set(value) => set_terminal_setting(original, value)?,
        SettingsMutation::Remove(key) => remove_terminal_setting(original, key)?,
    };
    document(changed.as_bytes())?;
    Ok(changed)
}

#[cfg(windows)]
mod native {
    use super::*;
    use pulse_platform::windows::{FileSnapshot, PrivateDirectory, PrivateFileError};
    use std::path::PathBuf;

    fn platform_error(error: PrivateFileError) -> Error {
        match error {
            PrivateFileError::Location => Error::Location,
            PrivateFileError::Revoked => Error::Authorization,
            PrivateFileError::NotFound | PrivateFileError::Io => Error::Io,
            PrivateFileError::Busy => Error::Busy,
            PrivateFileError::Conflict => Error::Conflict,
            PrivateFileError::Limit => SettingsError::Size.into(),
            PrivateFileError::Uncertain => Error::Uncertain,
        }
    }

    // No Debug/serde: exact document comments must not enter diagnostic logs.
    #[derive(Clone, PartialEq, Eq)]
    pub(crate) struct Snapshot {
        file: FileSnapshot,
        text: String,
    }
    impl Snapshot {
        pub(crate) fn text(&self) -> &str {
            &self.text
        }
        pub(crate) fn exists(&self) -> bool {
            self.file.exists()
        }
        fn decode(file: FileSnapshot) -> Result<Self> {
            let text = document(file.bytes())?.to_owned();
            Ok(Self { file, text })
        }
    }

    /// Synchronous: caller must keep its physical I/O reservation until this operation exits.
    /// Only an existing admitted directory is accepted; no permission repair or default reset.
    pub(crate) struct SettingsFile {
        directory: PrivateDirectory,
    }
    impl SettingsFile {
        pub(crate) fn open_private(path: PathBuf) -> Result<Self> {
            Ok(Self {
                directory: PrivateDirectory::open(path).map_err(|_| Error::Location)?,
            })
        }
        pub(crate) fn load(&self) -> Result<Snapshot> {
            let slot = self
                .directory
                .atomic_file("settings.toml", SETTINGS_TEXT_LIMIT)
                .map_err(platform_error)?;
            Snapshot::decode(slot.load().map_err(platform_error)?)
        }
        /// Only typed edits, checked under the original caller's authorization; no raw text save.
        /// Post-commit uncertainty requires reload. No directory crash-durability guarantee.
        pub(crate) fn save_authorized(
            &self,
            expected: &Snapshot,
            mutation: SettingsMutation,
            authorize: impl Fn() -> bool,
        ) -> Result<Snapshot> {
            let allowed = || {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(&authorize)).unwrap_or(false)
            };
            if !allowed() {
                return Err(Error::Authorization);
            }
            let text = edit(expected.text(), mutation)?;
            let slot = self
                .directory
                .atomic_file("settings.toml", SETTINGS_TEXT_LIMIT)
                .map_err(platform_error)?;
            let installed = slot
                .save(&expected.file, text.as_bytes(), allowed)
                .map_err(platform_error)?;
            Snapshot::decode(installed).map_err(|_| Error::Uncertain)
        }
    }
}

#[cfg(windows)]
pub(super) use native::{SettingsFile, Snapshot as SettingsSnapshot};

#[cfg(all(windows, test))]
#[path = "settings_file_windows_native_tests.rs"]
mod native_tests;

#[cfg(test)]
#[path = "settings_file_windows_document_tests.rs"]
mod document_tests;
