//! Bounded, reader-bound source snapshots with trusted synchronous filesystem adapters.
use crate::config::{
    ConfigBundle, ConfigError, ConfigQuery, MAX_DEPTH, MAX_SOURCE, MAX_SOURCES, MAX_TOTAL,
    ProvidedConfigSource, ResolvedConfig,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    sync::Arc,
};

/// Trusted backend filesystem, never a page-supplied path or execution capability.
/// Opening must revalidate the original directory identity and all platform admission rules.
/// Every operation is synchronous; the caller retains native I/O credit through actual exit.
pub trait ConfigSourceFilesystem: Send + Sync + 'static {
    /// A directory lease that keeps admitted ancestors alive until its last operation exits.
    type Directory: ConfigSourceDirectory;
    /// Reopen only the originally selected directory; replacement must be rejected.
    fn open(&self) -> Result<Self::Directory, ConfigError>;
}
/// Read-only directory operations provided only by trusted platform adapters.
/// Implementations reject links, unsafe permissions and nonregular or multiply linked files.
pub trait ConfigSourceDirectory: Sized + Send + Sync + 'static {
    /// Exact identity and mutation metadata; not content, authorization or a serialized token.
    type Stamp: Clone + Eq + Send + Sync + 'static;
    /// Revalidate this directory and capture exact metadata.
    fn stamp(&self) -> Result<Self::Stamp, ConfigError>;
    /// Duplicate this lease without losing its admitted identity or ancestors.
    fn duplicate(&self) -> Result<Self, ConfigError>;
    /// Open one relative component, preserving all original ancestors.
    fn child(&self, name: &str) -> Result<Self, ConfigError>;
    /// Enumerate names only, enforcing both budgets while iterating; omit dot entries.
    fn members(&self, entries: usize, bytes: usize) -> Result<Vec<Vec<u8>>, ConfigError>;
    /// Read one regular basename, enforcing the byte limit and checking before/after metadata.
    fn source(
        &self,
        name: &str,
        limit: usize,
    ) -> Result<ConfigSourceFile<Self::Stamp>, ConfigError>;
}
/// An adapter's bounded bytes and exact native identity/mutation stamp. No Debug or serialization.
pub struct ConfigSourceFile<S> {
    /// The selected source bytes; never identity-file contents obtained from a hint.
    pub bytes: Vec<u8>,
    /// Native metadata for the same read lease, captured after a stable bounded read.
    pub stamp: S,
}
type StampOf<B> = <<B as ConfigSourceFilesystem>::Directory as ConfigSourceDirectory>::Stamp;
struct Source<S> {
    name: String,
    bytes: Vec<u8>,
    stamp: S,
}
/// Original backend directory and reader identity; opening grants no page or SSH authority.
pub struct SourceDirectoryReader<B: ConfigSourceFilesystem> {
    filesystem: B,
    instance: Arc<()>,
}
/// Immutable captured sources bound to the original reader and, for discovery, exact query.
/// No Debug, serialization or clone; confirmation is not a lasting freshness guarantee.
pub struct SourceFileSnapshot<B: ConfigSourceFilesystem> {
    bundle: ConfigBundle,
    sources: Vec<Source<StampOf<B>>>,
    dirs: BTreeMap<String, StampOf<B>>,
    members: BTreeMap<String, Vec<Vec<u8>>>,
    instance: Arc<()>,
    query: Option<QueryBinding>,
}
impl<B: ConfigSourceFilesystem> SourceFileSnapshot<B> {
    /// Resolve only the original query when capture used automatic discovery.
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
impl<B: ConfigSourceFilesystem> SourceDirectoryReader<B> {
    /// Bind one trusted backend adapter; its open operation must enforce original identity.
    pub fn from_filesystem(filesystem: B) -> Self {
        Self {
            filesystem,
            instance: Arc::new(()),
        }
    }
    fn current(&self) -> Result<B::Directory, ConfigError> {
        self.filesystem.open()
    }
    /// Capture fixed config plus at most 32 backend-approved relative Include names.
    /// The catalog is never accepted from the page; all source and directory budgets apply.
    pub fn capture(
        &self,
        approved_includes: &[&str],
    ) -> Result<SourceFileSnapshot<B>, ConfigError> {
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
        let mut dirs = BTreeMap::from([(String::new(), opened.stamp()?)]);
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
        let snapshot = SourceFileSnapshot {
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
    /// Discover metadata only, then read only sources selected by the exact active query.
    pub fn capture_discovered(
        &self,
        query: &ConfigQuery<'_>,
    ) -> Result<SourceFileSnapshot<B>, ConfigError> {
        discovery::capture(self, query)
    }
    /// Revalidate source bytes, identities, directory stamps and membership before and after reads.
    /// This is not an atomic filesystem snapshot, database transaction or SSH authorization.
    pub fn confirm(&self, snapshot: &SourceFileSnapshot<B>) -> Result<(), ConfigError> {
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
fn relative_dir<D: ConfigSourceDirectory>(root: &D, name: &str) -> Result<D, ConfigError> {
    let mut current = root.duplicate()?;
    if !name.is_empty() {
        for component in name.split('/') {
            current = current.child(component)?;
        }
    }
    Ok(current)
}
fn check_dirs<D: ConfigSourceDirectory>(
    root: &D,
    expected: &BTreeMap<String, D::Stamp>,
) -> Result<(), ConfigError> {
    for (name, expected_stamp) in expected {
        if relative_dir(root, name)?.stamp()? != *expected_stamp {
            return Err(ConfigError::SourceChanged);
        }
    }
    Ok(())
}
fn read_members<D: ConfigSourceDirectory>(
    root: &D,
    dirs: &BTreeMap<String, D::Stamp>,
) -> Result<BTreeMap<String, Vec<Vec<u8>>>, ConfigError> {
    let mut all = BTreeMap::new();
    let mut count: usize = 0;
    let mut bytes: usize = 0;
    for name in dirs.keys() {
        let opened = relative_dir(root, name)?;
        let mut names = opened.members(4096 - count, MAX_TOTAL - bytes)?;
        for value in &names {
            count = count.checked_add(1).ok_or(ConfigError::Limit)?;
            bytes = bytes.checked_add(value.len()).ok_or(ConfigError::Limit)?;
            if count > 4096 || bytes > MAX_TOTAL {
                return Err(ConfigError::Limit);
            }
        }
        names.sort();
        all.insert(name.clone(), names);
    }
    Ok(all)
}
fn read_source<D: ConfigSourceDirectory>(
    root: &D,
    name: &str,
    dirs: &mut BTreeMap<String, D::Stamp>,
    total: &mut usize,
) -> Result<Source<D::Stamp>, ConfigError> {
    valid_name(name)?;
    let mut current = root.duplicate()?;
    let mut parts = name.split('/').peekable();
    let mut parent_name = String::new();
    let mut leaf = "";
    while let Some(part) = parts.next() {
        if parts.peek().is_none() {
            leaf = part;
            break;
        }
        current = current.child(part)?;
        if !parent_name.is_empty() {
            parent_name.push('/');
        }
        parent_name.push_str(part);
        let current_stamp = current.stamp()?;
        if let Some(previous) = dirs.insert(parent_name.clone(), current_stamp.clone())
            && previous != current_stamp
        {
            return Err(ConfigError::SourceChanged);
        }
    }
    let limit = MAX_SOURCE.min(MAX_TOTAL.checked_sub(*total).ok_or(ConfigError::Limit)?);
    let result = current.source(leaf, limit)?;
    if result.bytes.len() > limit {
        return Err(ConfigError::Limit);
    }
    *total = total
        .checked_add(result.bytes.len())
        .ok_or(ConfigError::Limit)?;
    Ok(Source {
        name: name.to_owned(),
        bytes: result.bytes,
        stamp: result.stamp,
    })
}
#[path = "config_discovery.rs"]
mod discovery;
use discovery::QueryBinding;
#[cfg(unix)]
#[path = "config_source_unix.rs"]
mod native;
/// Unix backend reader, retaining descriptor-relative admission and original identity checks.
#[cfg(unix)]
pub type ConfigDirectoryReader = SourceDirectoryReader<native::UnixConfigFilesystem>;
/// Immutable Unix source snapshot; no Debug, serialization or connection authority.
#[cfg(unix)]
pub type ConfigFileSnapshot = SourceFileSnapshot<native::UnixConfigFilesystem>;
#[cfg(all(test, unix))]
use crate::config::file::stamp;
#[cfg(all(test, unix))]
use std::path::Path;
#[cfg(all(test, unix))]
#[path = "config_discovery_tests.rs"]
mod discovery_tests;
#[cfg(all(test, unix))]
#[path = "config_source_tests.rs"]
mod tests;
