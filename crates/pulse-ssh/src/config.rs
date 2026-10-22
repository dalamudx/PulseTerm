//! Bounded OpenSSH metadata resolution and reader-bound read-only sources; no execution.
use crate::HostTarget;
use std::collections::BTreeMap;
use std::sync::Arc;
#[path = "config_resolution.rs"]
mod resolution;
use resolution::{ProvidedSources, SourceProvider, resolve_directives};
#[path = "config_syntax.rs"]
mod syntax;
use syntax::{Criterion, Directive, parse, path_pattern, pattern_list};
const MAX_SOURCE: usize = 256 * 1024;
const MAX_TOTAL: usize = 512 * 1024;
const MAX_LINES: usize = 4096;
const MAX_SOURCES: usize = 32;
const MAX_DEPTH: usize = 8;
const MAX_MATCH_STEPS: usize = 1_000_000;

/// Content-free parse/resolution errors; no source text, filenames or command strings.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ConfigError {
    /// The input or resolution work exceeded a fixed budget.
    #[error("ssh.config.limit")]
    Limit,
    /// Invalid UTF-8, control characters, quoting or option syntax.
    #[error("ssh.config.syntax")]
    Syntax,
    /// An active option or condition is outside this metadata subset.
    #[error("ssh.config.unsupported")]
    Unsupported,
    /// An active Include has no explicitly supplied matching source.
    #[error("ssh.config.include_missing")]
    MissingInclude,
    /// An active Include cycle was encountered.
    #[error("ssh.config.include_cycle")]
    IncludeCycle,
    /// The resolved endpoint or username is not representable.
    #[error("ssh.config.invalid_target")]
    InvalidTarget,
    /// A fixed primary source is missing, unsafe, unsupported on this platform or unreadable.
    #[error("ssh.config.source_unavailable")]
    SourceUnavailable,
    /// The reader instance, source content or directory identity changed.
    #[error("ssh.config.source_changed")]
    SourceChanged,
}

/// One caller-supplied Include name and its already-read bytes. It grants no filesystem access.
pub struct ProvidedConfigSource<'a> {
    name: &'a str,
    bytes: &'a [u8],
}
impl<'a> ProvidedConfigSource<'a> {
    /// Supply an inert ASCII name; callers remain responsible for secure file admission.
    pub fn new(name: &'a str, bytes: &'a [u8]) -> Self {
        Self { name, bytes }
    }
}

/// Original lookup inputs; no ambient username, filesystem, DNS or environment lookup.
pub struct ConfigQuery<'a> {
    alias: &'a str,
    local_user: &'a str,
    user: Option<&'a str>,
    port: Option<u16>,
}
impl<'a> ConfigQuery<'a> {
    /// Explicit user/port override file values, as command-line options do in OpenSSH.
    pub fn new(
        alias: &'a str,
        local_user: &'a str,
        user: Option<&'a str>,
        port: Option<u16>,
    ) -> Result<Self, ConfigError> {
        if alias.is_empty()
            || alias.len() > 253
            || !alias.is_ascii()
            || !alias
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"-._:".contains(&b))
            || !valid_user(local_user)
            || user.is_some_and(|u| !valid_user(u))
            || port == Some(0)
        {
            return Err(ConfigError::InvalidTarget);
        }
        Ok(Self {
            alias,
            local_user,
            user,
            port,
        })
    }
}
fn valid_user(user: &str) -> bool {
    !user.is_empty()
        && user.len() <= 128
        && user.is_ascii()
        && user
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-._@\\".contains(&b))
}

/// Parsed text bundle. Include expansion only searches these explicit names, never disk.
pub struct ConfigBundle {
    primary: Vec<Directive>,
    sources: BTreeMap<String, Arc<Vec<Directive>>>,
}
impl ConfigBundle {
    /// Bound aggregate bytes/lines and reject duplicate names before resolving any query.
    pub fn parse(
        primary: &[u8],
        sources: &[ProvidedConfigSource<'_>],
    ) -> Result<Self, ConfigError> {
        if sources.len() > MAX_SOURCES {
            return Err(ConfigError::Limit);
        }
        let mut bytes = 0;
        let mut lines = 0;
        let primary = parse(primary, &mut bytes, &mut lines)?;
        let mut parsed = BTreeMap::new();
        for source in sources {
            if source.name.is_empty()
                || source.name.len() > 512
                || !source.name.is_ascii()
                || source.name.chars().any(char::is_control)
                || source.name.contains(['*', '?', '[', ']', '%'])
            {
                return Err(ConfigError::Syntax);
            }
            if parsed.contains_key(source.name) {
                return Err(ConfigError::Syntax);
            }
            parsed.insert(
                source.name.to_owned(),
                Arc::new(parse(source.bytes, &mut bytes, &mut lines)?),
            );
        }
        Ok(Self {
            primary,
            sources: parsed,
        })
    }
    /// Resolve one alias with first-value semantics and bounded textual Include processing.
    /// This neither imports an identity nor approves a fingerprint or authorizes a connection.
    pub fn resolve(&self, query: &ConfigQuery<'_>) -> Result<ResolvedConfig, ConfigError> {
        resolve_directives(&self.primary, query, &mut ProvidedSources(&self.sources))
    }
}
fn walk<P: SourceProvider>(
    provider: &mut P,
    directives: &[Directive],
    state: &mut Resolution<'_, '_>,
) -> Result<(), ConfigError> {
    for directive in directives {
        state.directives_left = state
            .directives_left
            .checked_sub(1)
            .ok_or(ConfigError::Limit)?;
        match directive {
            Directive::Host(patterns) => {
                state.active =
                    pattern_list(patterns, state.query.alias, false, &mut state.match_steps)?
            }
            Directive::Match(criteria) => {
                let mut active = true;
                for criterion in criteria {
                    let matched = match criterion {
                        Criterion::All => true,
                        Criterion::Host(patterns) => pattern_list(
                            patterns,
                            state.host.as_deref().unwrap_or(state.query.alias),
                            true,
                            &mut state.match_steps,
                        )?,
                        Criterion::OriginalHost(patterns) => {
                            pattern_list(patterns, state.query.alias, true, &mut state.match_steps)?
                        }
                        Criterion::User(patterns) => pattern_list(
                            patterns,
                            state.user.as_deref().unwrap_or(state.query.local_user),
                            false,
                            &mut state.match_steps,
                        )?,
                        Criterion::LocalUser(patterns) => pattern_list(
                            patterns,
                            state.query.local_user,
                            false,
                            &mut state.match_steps,
                        )?,
                        Criterion::Unsupported => return Err(ConfigError::Unsupported),
                    };
                    active &= matched;
                }
                state.active = active;
            }
            Directive::Include(patterns) if state.active => {
                for pattern in patterns {
                    let names = provider.matching(pattern, &mut state.match_steps)?;
                    if names.is_empty() {
                        return Err(ConfigError::MissingInclude);
                    }
                    for name in names {
                        if state.stack.contains(&name) {
                            return Err(ConfigError::IncludeCycle);
                        }
                        if state.stack.len() == MAX_DEPTH {
                            return Err(ConfigError::Limit);
                        }
                        let included = provider.load(&name)?;
                        let outer_active = state.active;
                        state.stack.push(name);
                        walk(provider, &included, state)?;
                        state.stack.pop();
                        state.active = outer_active;
                    }
                }
            }
            Directive::HostName(host) if state.active && state.host.is_none() => {
                let mut expanded = String::new();
                let mut chars = host.chars();
                while let Some(c) = chars.next() {
                    if c == '%' {
                        match chars.next() {
                            Some('h') => expanded.push_str(state.query.alias),
                            Some('%') => expanded.push('%'),
                            _ => return Err(ConfigError::Unsupported),
                        }
                    } else {
                        expanded.push(c);
                    }
                    if expanded.len() > 253 {
                        return Err(ConfigError::Limit);
                    }
                }
                state.host = Some(expanded);
            }
            Directive::Port(port) if state.active && state.port.is_none() => {
                state.port = Some(*port)
            }
            Directive::User(user) if state.active && state.user.is_none() => {
                state.user = Some(user.clone())
            }
            Directive::IdentityFile(path)
                if state.active && !state.identity_files.contains(path) =>
            {
                if state.identity_files.len() == 32 {
                    return Err(ConfigError::Limit);
                }
                state.identity_files.push(path.clone());
            }
            Directive::IdentitiesOnly(value) if state.active && state.identities_only.is_none() => {
                state.identities_only = Some(*value)
            }
            Directive::Unsupported if state.active => return Err(ConfigError::Unsupported),
            _ => {}
        }
    }
    Ok(())
}
struct Resolution<'a, 'b> {
    query: &'a ConfigQuery<'b>,
    active: bool,
    host: Option<String>,
    port: Option<u16>,
    user: Option<String>,
    identity_files: Vec<String>,
    identities_only: Option<bool>,
    directives_left: usize,
    match_steps: usize,
    stack: Vec<String>,
}

/// Drawing/import metadata only, deliberately without Debug or serialization implementations.
pub struct ResolvedConfig {
    target: HostTarget,
    user: String,
    identity_files: Vec<String>,
    identities_only: Option<bool>,
}
impl ResolvedConfig {
    /// Canonical endpoint; possession does not approve a peer or authorize a dial.
    pub fn target(&self) -> &HostTarget {
        &self.target
    }
    /// Effective explicit or inherited username.
    pub fn user(&self) -> &str {
        &self.user
    }
    /// Ordered, deduplicated literal IdentityFile hints, including `none` as an inert marker.
    /// No tilde/token expansion or filesystem access; never a key-read capability.
    pub fn identity_files(&self) -> &[String] {
        &self.identity_files
    }
    /// Parsed hint only; no Agent or credential-use authorization is granted.
    pub fn identities_only(&self) -> Option<bool> {
        self.identities_only
    }
}
// rustix does not expose mkfifoat on macOS; keep the same FIFO fixtures on Unix.
#[cfg(all(test, unix))]
fn mkfifo(path: &std::path::Path) -> std::io::Result<()> {
    let status = std::process::Command::new("/usr/bin/mkfifo")
        .args(["-m", "600"])
        .arg(path)
        .status()?;
    if !status.success() {
        return Err(std::io::Error::other("config.fifo_fixture_failed"));
    }
    Ok(())
}

#[cfg(all(test, unix))]
#[path = "config_openssh_tests.rs"]
mod openssh_tests;
#[cfg(test)]
#[path = "config_tests.rs"]
mod tests;

#[cfg(unix)]
#[path = "config_file.rs"]
mod file;
#[path = "config_source.rs"]
mod sources;
#[cfg(unix)]
pub use sources::{ConfigDirectoryReader, ConfigFileSnapshot};
pub use sources::{
    ConfigSourceDirectory, ConfigSourceFile, ConfigSourceFilesystem, SourceDirectoryReader,
    SourceFileSnapshot,
};
/// Read only the fixed `config` in a backend-selected private SSH directory.
/// Reject missing/unsafe files without writes. Include texts must be separately supplied;
/// no Include file, identity key, environment variable or home directory is read implicitly.
/// This synchronous IO must retain the caller's native credit through actual completion.
/// Before/after descriptor checks detect common replacements, not complete TOCTOU or freshness.
#[cfg(unix)]
pub fn load_primary_config_directory(
    ssh_dir: &std::path::Path,
    sources: &[ProvidedConfigSource<'_>],
) -> Result<ConfigBundle, ConfigError> {
    file::load(ssh_dir, sources)
}
/// Unsupported platforms fail explicitly; never report an empty configuration as a successful read.
#[cfg(not(unix))]
pub fn load_primary_config_directory(
    _: &std::path::Path,
    _: &[ProvidedConfigSource<'_>],
) -> Result<ConfigBundle, ConfigError> {
    Err(ConfigError::SourceUnavailable)
}
