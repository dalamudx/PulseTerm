//! Explicitly admitted Include files and reader-bound source confirmation.
use super::{File, Metadata, OPEN, directory, private_directory, regular, stamp};
use crate::config::{
    ConfigBundle, ConfigError, ConfigQuery, MAX_DEPTH, MAX_SOURCE, MAX_SOURCES, MAX_TOTAL,
    ProvidedConfigSource, ResolvedConfig,
};
use rustix::fs::{self as sys, Mode, OFlags};
use std::os::unix::fs::MetadataExt;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::Read,
    path::{Path, PathBuf},
    sync::Arc,
};
type Stamp = (u64, u64, u64, i64, i64, i64, i64);
struct Source {
    name: String,
    bytes: Vec<u8>,
    stamp: Stamp,
}
/// Backend-selected private directory. Opening it grants neither page nor connection authority.
/// Every operation is synchronous: callers retain admission through the actual blocking exit.
pub struct ConfigDirectoryReader {
    path: PathBuf,
    identity: (u64, u64),
    instance: Arc<()>,
}
/// Immutable metadata and sources captured by exactly one reader instance.
/// No Debug, serialization or clone; resolve is inert and confirmation grants no SSH authority.
pub struct ConfigFileSnapshot {
    bundle: ConfigBundle,
    sources: Vec<Source>,
    dirs: BTreeMap<String, Stamp>,
    members: BTreeMap<String, Vec<Vec<u8>>>,
    instance: Arc<()>,
    query: Option<QueryBinding>,
}
impl ConfigFileSnapshot {
    /// Resolve the immutable captured text; this does not establish current source freshness.
    pub fn resolve(&self, query: &ConfigQuery<'_>) -> Result<ResolvedConfig, ConfigError> {
        if self
            .query
            .as_ref()
            .is_some_and(|original| !original.matches(query))
        {
            return Err(ConfigError::SourceChanged);
        }
        self.bundle.resolve(query)
    }
}
impl ConfigDirectoryReader {
    /// Select a private backend directory with no-follow admission for every parent component.
    pub fn open(path: &Path) -> Result<Self, ConfigError> {
        let opened = directory(path)?;
        let stat = metadata(&opened)?;
        Ok(Self {
            path: path.to_owned(),
            identity: (stat.dev(), stat.ino()),
            instance: Arc::new(()),
        })
    }
    fn current(&self) -> Result<File, ConfigError> {
        let opened = directory(&self.path)?;
        let stat = metadata(&opened)?;
        if (stat.dev(), stat.ino()) != self.identity {
            return Err(ConfigError::SourceChanged);
        }
        Ok(opened)
    }
    /// Read fixed `config` plus at most 32 explicitly approved relative Include names.
    /// Wildcards do not discover files; names are a backend admission catalog, not page paths.
    /// All nested directories remain private, regular files single-linked and byte/work bounded.
    pub fn capture(&self, approved_includes: &[&str]) -> Result<ConfigFileSnapshot, ConfigError> {
        if approved_includes.len() > MAX_SOURCES {
            return Err(ConfigError::Limit);
        }
        let mut unique = BTreeSet::new();
        for name in approved_includes {
            valid_name(name)?;
            if *name == "config" || !unique.insert(*name) {
                return Err(ConfigError::Syntax);
            }
        }
        let opened = self.current()?;
        let mut dirs = BTreeMap::from([(String::new(), stamp(&metadata(&opened)?))]);
        let mut total = 0;
        let mut sources = Vec::with_capacity(approved_includes.len() + 1);
        for name in std::iter::once("config").chain(approved_includes.iter().copied()) {
            sources.push(read_source(&opened, name, &mut dirs, &mut total)?);
        }
        let provided: Vec<_> = sources
            .iter()
            .skip(1)
            .map(|s| ProvidedConfigSource::new(&s.name, &s.bytes))
            .collect();
        let bundle = ConfigBundle::parse(&sources[0].bytes, &provided)?;
        let members = read_members(&opened, &dirs)?;
        let snapshot = ConfigFileSnapshot {
            bundle,
            sources,
            dirs,
            members,
            instance: self.instance.clone(),
            query: None,
        };
        self.confirm(&snapshot)?;
        Ok(snapshot)
    }
    /// Discover only active relative Includes using the same bounded Host/Match interpreter.
    /// IdentityFile hints cause no reads. Sources are bound to this query and the original reader.
    /// Missing, unsafe or unsupported matches reject; this never grants key or connection access.
    pub fn capture_discovered(
        &self,
        query: &ConfigQuery<'_>,
    ) -> Result<ConfigFileSnapshot, ConfigError> {
        discovery::capture(self, query)
    }
    /// Recheck the original instance, all source bytes and stamps, and traversed directory stamps.
    /// Any failed comparison is a rejection; this is not a lock, atomic snapshot or lasting grant.
    pub fn confirm(&self, snapshot: &ConfigFileSnapshot) -> Result<(), ConfigError> {
        if !Arc::ptr_eq(&self.instance, &snapshot.instance) {
            return Err(ConfigError::SourceChanged);
        }
        let opened = self.current()?;
        check_dirs(&opened, &snapshot.dirs)?;
        if read_members(&opened, &snapshot.dirs)? != snapshot.members {
            return Err(ConfigError::SourceChanged);
        }
        let mut observed = BTreeMap::new();
        let mut total = 0;
        for expected in &snapshot.sources {
            let actual = read_source(&opened, &expected.name, &mut observed, &mut total)?;
            if actual.stamp != expected.stamp || actual.bytes != expected.bytes {
                return Err(ConfigError::SourceChanged);
            }
        }
        check_dirs(&opened, &snapshot.dirs)?;
        let current = self.current()?;
        check_dirs(&current, &snapshot.dirs)?;
        if read_members(&current, &snapshot.dirs)? != snapshot.members {
            return Err(ConfigError::SourceChanged);
        }
        Ok(())
    }
}
fn metadata(file: &File) -> Result<Metadata, ConfigError> {
    file.metadata().map_err(|_| ConfigError::SourceUnavailable)
}
fn valid_name(name: &str) -> Result<(), ConfigError> {
    if name.is_empty()
        || name.len() > 512
        || !name.is_ascii()
        || name
            .bytes()
            .any(|c| c.is_ascii_control() || c.is_ascii_whitespace())
        || name.contains(['*', '?', '[', ']', '%', '\\'])
        || name
            .split('/')
            .any(|p| p.is_empty() || p == "." || p == "..")
    {
        return Err(ConfigError::Syntax);
    }
    if name.split('/').count() > MAX_DEPTH {
        return Err(ConfigError::Limit);
    }
    Ok(())
}
fn child(parent: &File, name: &str) -> Result<File, ConfigError> {
    let opened: File = sys::openat(parent, name, OPEN | OFlags::DIRECTORY, Mode::empty())
        .map_err(|_| ConfigError::SourceUnavailable)?
        .into();
    if !private_directory(&metadata(&opened)?) {
        return Err(ConfigError::SourceUnavailable);
    }
    Ok(opened)
}
fn relative_dir(root: &File, name: &str) -> Result<File, ConfigError> {
    let mut current = root
        .try_clone()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    if !name.is_empty() {
        for component in name.split('/') {
            current = child(&current, component)?;
        }
    }
    Ok(current)
}
fn check_dirs(root: &File, expected: &BTreeMap<String, Stamp>) -> Result<(), ConfigError> {
    for (name, expected_stamp) in expected {
        if stamp(&metadata(&relative_dir(root, name)?)?) != *expected_stamp {
            return Err(ConfigError::SourceChanged);
        }
    }
    Ok(())
}
fn read_members(
    root: &File,
    dirs: &BTreeMap<String, Stamp>,
) -> Result<BTreeMap<String, Vec<Vec<u8>>>, ConfigError> {
    let mut all = BTreeMap::new();
    let mut count: usize = 0;
    let mut bytes: usize = 0;
    for name in dirs.keys() {
        let opened = relative_dir(root, name)?;
        let mut entries =
            sys::Dir::read_from(&opened).map_err(|_| ConfigError::SourceUnavailable)?;
        let mut names = Vec::new();
        while let Some(entry) = entries.read() {
            let entry = entry.map_err(|_| ConfigError::SourceUnavailable)?;
            let value = entry.file_name().to_bytes();
            if value == b"." || value == b".." {
                continue;
            }
            count = count.checked_add(1).ok_or(ConfigError::Limit)?;
            bytes = bytes.checked_add(value.len()).ok_or(ConfigError::Limit)?;
            if count > 4096 || bytes > MAX_TOTAL {
                return Err(ConfigError::Limit);
            }
            names.push(value.to_vec());
        }
        names.sort();
        all.insert(name.clone(), names);
    }
    Ok(all)
}
fn read_source(
    root: &File,
    name: &str,
    dirs: &mut BTreeMap<String, Stamp>,
    total: &mut usize,
) -> Result<Source, ConfigError> {
    let mut current = root
        .try_clone()
        .map_err(|_| ConfigError::SourceUnavailable)?;
    let mut parts = name.split('/').peekable();
    let mut parent_name = String::new();
    let mut leaf = "";
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            leaf = part;
            break;
        }
        current = child(&current, part)?;
        if !parent_name.is_empty() {
            parent_name.push('/');
        }
        parent_name.push_str(part);
        let current_stamp = stamp(&metadata(&current)?);
        if let Some(previous) = dirs.insert(parent_name.clone(), current_stamp)
            && previous != current_stamp
        {
            return Err(ConfigError::SourceChanged);
        }
    }
    let file: File = sys::openat(&current, leaf, OPEN, Mode::empty())
        .map_err(|_| ConfigError::SourceUnavailable)?
        .into();
    let before = metadata(&file)?;
    if !regular(&before) {
        return Err(ConfigError::SourceUnavailable);
    }
    let remaining = MAX_TOTAL.checked_sub(*total).ok_or(ConfigError::Limit)?;
    if before.len() > remaining as u64 {
        return Err(ConfigError::Limit);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_SOURCE.min(remaining) as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ConfigError::SourceUnavailable)?;
    if bytes.len() > MAX_SOURCE.min(remaining) {
        return Err(ConfigError::Limit);
    }
    let after = metadata(&file)?;
    if !regular(&after) || stamp(&before) != stamp(&after) {
        return Err(ConfigError::SourceChanged);
    }
    *total = total.checked_add(bytes.len()).ok_or(ConfigError::Limit)?;
    Ok(Source {
        name: name.to_owned(),
        bytes,
        stamp: stamp(&after),
    })
}
#[path = "config_discovery.rs"]
mod discovery;
use discovery::QueryBinding;
#[cfg(test)]
#[path = "config_discovery_tests.rs"]
mod discovery_tests;
#[cfg(test)]
#[path = "config_source_tests.rs"]
mod tests;
