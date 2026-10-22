//! Lazy, descriptor-relative Include admission. No IdentityFile lookup or connection authority.
use super::*;
use crate::config::{
    path_pattern,
    resolution::{SourceProvider, resolve_directives},
    syntax::{Directive, parse},
};

pub(super) struct QueryBinding {
    alias: String,
    local_user: String,
    user: Option<String>,
    port: Option<u16>,
}
impl QueryBinding {
    pub(super) fn matches(&self, query: &ConfigQuery<'_>) -> bool {
        self.alias == query.alias
            && self.local_user == query.local_user
            && self.user.as_deref() == query.user
            && self.port == query.port
    }
}
struct Discovered<D: ConfigSourceDirectory> {
    root: D,
    sources: Vec<Source<D::Stamp>>,
    parsed: BTreeMap<String, Arc<Vec<Directive>>>,
    dirs: BTreeMap<String, D::Stamp>,
    members: BTreeMap<String, Vec<Vec<u8>>>,
    entries: usize,
    name_bytes: usize,
    source_bytes: usize,
    parse_bytes: usize,
    parse_lines: usize,
}
impl<D: ConfigSourceDirectory> Discovered<D> {
    fn directory_members(&mut self, name: &str) -> Result<Vec<Vec<u8>>, ConfigError> {
        if let Some(names) = self.members.get(name) {
            return Ok(names.clone());
        }
        if self.dirs.len() >= MAX_SOURCES * MAX_DEPTH {
            return Err(ConfigError::Limit);
        }
        let opened = relative_dir(&self.root, name)?;
        let before = opened.stamp()?;
        let mut names = opened.members(4096 - self.entries, MAX_TOTAL - self.name_bytes)?;
        for value in &names {
            self.entries = self.entries.checked_add(1).ok_or(ConfigError::Limit)?;
            self.name_bytes = self
                .name_bytes
                .checked_add(value.len())
                .ok_or(ConfigError::Limit)?;
            if self.entries > 4096 || self.name_bytes > MAX_TOTAL {
                return Err(ConfigError::Limit);
            }
        }
        if opened.stamp()? != before {
            return Err(ConfigError::SourceChanged);
        }
        if self
            .dirs
            .insert(name.to_owned(), before.clone())
            .is_some_and(|previous| previous != before)
        {
            return Err(ConfigError::SourceChanged);
        }
        names.sort();
        self.members.insert(name.to_owned(), names.clone());
        Ok(names)
    }
}
impl<D: ConfigSourceDirectory> SourceProvider for Discovered<D> {
    fn matching(&mut self, pattern: &str, steps: &mut usize) -> Result<Vec<String>, ConfigError> {
        if pattern.is_empty()
            || pattern.len() > 512
            || !pattern.is_ascii()
            || pattern
                .bytes()
                .any(|b| b.is_ascii_control() || b.is_ascii_whitespace())
            || pattern.contains(['[', ']', '%', '\\', '~'])
            || pattern
                .split('/')
                .any(|part| part.is_empty() || part == "." || part == "..")
        {
            return Err(ConfigError::Unsupported);
        }
        let parts: Vec<_> = pattern.split('/').collect();
        if parts.len() > MAX_DEPTH {
            return Err(ConfigError::Limit);
        }
        let mut prefixes = vec![String::new()];
        for (index, part) in parts.iter().enumerate() {
            let mut matched = Vec::new();
            for prefix in prefixes {
                for bytes in self.directory_members(&prefix)? {
                    let name = std::str::from_utf8(&bytes).map_err(|_| ConfigError::Unsupported)?;
                    if !path_pattern(part, name, steps)? {
                        continue;
                    }
                    let joined = if prefix.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{prefix}/{name}")
                    };
                    valid_name(&joined)?;
                    if index + 1 != parts.len() {
                        self.directory_members(&joined)?;
                    }
                    matched.push(joined);
                }
            }
            matched.sort();
            matched.dedup();
            prefixes = matched;
        }
        Ok(prefixes)
    }
    fn load(&mut self, name: &str) -> Result<Arc<Vec<Directive>>, ConfigError> {
        if let Some(parsed) = self.parsed.get(name) {
            return Ok(parsed.clone());
        }
        if self.sources.len() > MAX_SOURCES {
            return Err(ConfigError::Limit);
        }
        let source = read_source(&self.root, name, &mut self.dirs, &mut self.source_bytes)?;
        let parsed = Arc::new(parse(
            &source.bytes,
            &mut self.parse_bytes,
            &mut self.parse_lines,
        )?);
        self.parsed.insert(name.to_owned(), parsed.clone());
        self.sources.push(source);
        Ok(parsed)
    }
}
pub(super) fn capture<B: ConfigSourceFilesystem>(
    reader: &SourceDirectoryReader<B>,
    query: &ConfigQuery<'_>,
) -> Result<SourceFileSnapshot<B>, ConfigError> {
    let root = reader.current()?;
    let mut dirs = BTreeMap::from([(String::new(), root.stamp()?)]);
    let mut source_bytes = 0;
    let source = read_source(&root, "config", &mut dirs, &mut source_bytes)?;
    let mut parse_bytes = 0;
    let mut parse_lines = 0;
    let primary = Arc::new(parse(&source.bytes, &mut parse_bytes, &mut parse_lines)?);
    let mut provider = Discovered {
        root,
        sources: vec![source],
        parsed: BTreeMap::from([("config".to_owned(), primary.clone())]),
        dirs,
        members: BTreeMap::new(),
        entries: 0,
        name_bytes: 0,
        source_bytes,
        parse_bytes,
        parse_lines,
    };
    resolve_directives(&primary, query, &mut provider)?;
    // Even without active Includes, record the primary directory membership for confirmation.
    provider.directory_members("")?;
    let supplied: Vec<_> = provider
        .sources
        .iter()
        .skip(1)
        .map(|source| ProvidedConfigSource::new(&source.name, &source.bytes))
        .collect();
    let bundle = ConfigBundle::parse(&provider.sources[0].bytes, &supplied)?;
    let snapshot = SourceFileSnapshot {
        bundle,
        sources: provider.sources,
        dirs: provider.dirs,
        members: provider.members,
        instance: reader.instance.clone(),
        query: Some(QueryBinding {
            alias: query.alias.to_owned(),
            local_user: query.local_user.to_owned(),
            user: query.user.map(str::to_owned),
            port: query.port,
        }),
    };
    reader.confirm(&snapshot)?;
    Ok(snapshot)
}
