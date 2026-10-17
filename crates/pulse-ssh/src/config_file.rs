//! Backend-selected fixed primary config read. Includes remain explicitly supplied text.
use super::{ConfigBundle, ConfigError, MAX_SOURCE, ProvidedConfigSource};
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    fs::{File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::RDONLY);
fn directory(path: &Path) -> Result<File, ConfigError> {
    if !path.is_absolute() {
        return Err(ConfigError::SourceUnavailable);
    }
    let mut current: File = sys::open("/", OPEN | OFlags::DIRECTORY, Mode::empty())
        .map_err(|_| ConfigError::SourceUnavailable)?
        .into();
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                current = sys::openat(&current, name, OPEN | OFlags::DIRECTORY, Mode::empty())
                    .map_err(|_| ConfigError::SourceUnavailable)?
                    .into();
            }
            _ => return Err(ConfigError::SourceUnavailable),
        }
    }
    if !private_directory(
        &current
            .metadata()
            .map_err(|_| ConfigError::SourceUnavailable)?,
    ) {
        return Err(ConfigError::SourceUnavailable);
    }
    Ok(current)
}
fn private_directory(metadata: &Metadata) -> bool {
    metadata.is_dir()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o077 == 0
        && metadata.mode() & 0o500 == 0o500
}
fn regular(metadata: &Metadata) -> bool {
    metadata.is_file()
        && metadata.uid() == rustix::process::geteuid().as_raw()
        && metadata.mode() & 0o022 == 0
        && metadata.mode() & 0o400 != 0
        && metadata.nlink() == 1
        && metadata.len() <= MAX_SOURCE as u64
}
fn stamp(metadata: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
        metadata.ctime(),
        metadata.ctime_nsec(),
    )
}
fn primary(directory: &File) -> Result<File, ConfigError> {
    sys::openat(directory, "config", OPEN, Mode::empty())
        .map(File::from)
        .map_err(|_| ConfigError::SourceUnavailable)
}
pub(super) fn load(
    path: &Path,
    sources: &[ProvidedConfigSource<'_>],
) -> Result<ConfigBundle, ConfigError> {
    let opened = directory(path)?;
    let before_dir = opened
        .metadata()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    let file = primary(&opened)?;
    let before = file
        .metadata()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    if !regular(&before) {
        return Err(ConfigError::SourceUnavailable);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_SOURCE as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError::SourceUnavailable)?;
    let after = file
        .metadata()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    let current = directory(path)?;
    let current_dir = current
        .metadata()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    let current_file = primary(&current)?
        .metadata()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    if bytes.len() > MAX_SOURCE
        || !regular(&after)
        || !regular(&current_file)
        || stamp(&before) != stamp(&after)
        || stamp(&after) != stamp(&current_file)
        || before_dir.dev() != current_dir.dev()
        || before_dir.ino() != current_dir.ino()
    {
        return Err(ConfigError::SourceUnavailable);
    }
    ConfigBundle::parse(&bytes, sources)
}
#[path = "config_source.rs"]
pub(super) mod sources;
#[cfg(test)]
#[path = "config_file_tests.rs"]
mod tests;
