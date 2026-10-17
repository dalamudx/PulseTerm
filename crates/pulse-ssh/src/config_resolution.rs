//! One bounded interpreter shared by inert text and lazily admitted disk sources.
use super::{
    ConfigError, ConfigQuery, Directive, HostTarget, MAX_LINES, MAX_MATCH_STEPS, Resolution,
    ResolvedConfig, path_pattern, valid_user, walk,
};
use std::{collections::BTreeMap, sync::Arc};
pub(super) trait SourceProvider {
    fn matching(&mut self, pattern: &str, steps: &mut usize) -> Result<Vec<String>, ConfigError>;
    fn load(&mut self, name: &str) -> Result<Arc<Vec<Directive>>, ConfigError>;
}
pub(super) struct ProvidedSources<'a>(pub &'a BTreeMap<String, Arc<Vec<Directive>>>);
impl SourceProvider for ProvidedSources<'_> {
    fn matching(&mut self, pattern: &str, steps: &mut usize) -> Result<Vec<String>, ConfigError> {
        let mut names = Vec::new();
        for name in self.0.keys() {
            if path_pattern(pattern, name, steps)? {
                names.push(name.clone());
            }
        }
        Ok(names)
    }
    fn load(&mut self, name: &str) -> Result<Arc<Vec<Directive>>, ConfigError> {
        self.0.get(name).cloned().ok_or(ConfigError::MissingInclude)
    }
}
pub(super) fn resolve_directives<P: SourceProvider>(
    primary: &[Directive],
    query: &ConfigQuery<'_>,
    provider: &mut P,
) -> Result<ResolvedConfig, ConfigError> {
    let mut state = Resolution {
        query,
        active: true,
        host: None,
        port: query.port,
        user: query.user.map(str::to_owned),
        identity_files: Vec::new(),
        identities_only: None,
        directives_left: MAX_LINES,
        match_steps: MAX_MATCH_STEPS,
        stack: Vec::new(),
    };
    walk(provider, primary, &mut state)?;
    let host = state.host.as_deref().unwrap_or(query.alias);
    let target =
        HostTarget::new(host, state.port.unwrap_or(22)).map_err(|_| ConfigError::InvalidTarget)?;
    let user = state.user.unwrap_or_else(|| query.local_user.to_owned());
    if !valid_user(&user) {
        return Err(ConfigError::InvalidTarget);
    }
    Ok(ResolvedConfig {
        target,
        user,
        identity_files: state.identity_files,
        identities_only: state.identities_only,
    })
}
