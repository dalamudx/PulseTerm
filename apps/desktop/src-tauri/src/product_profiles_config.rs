//! Original-page config sources; blocking work owns the shared read credit until exit.
use super::*;
#[cfg(windows)]
use crate::native_ssh_config_windows::{ConfigDirectoryReader, ConfigFileSnapshot, open_reader};
#[cfg(unix)]
use pulse_ssh::config::{ConfigDirectoryReader, ConfigFileSnapshot};
use pulse_ssh::config::{ConfigError, ConfigQuery, ResolvedConfig};
#[cfg(unix)]
fn open_reader(path: &std::path::Path) -> std::result::Result<ConfigDirectoryReader, ConfigError> {
    ConfigDirectoryReader::open(path)
}
use std::sync::Weak;

pub(in super::super) struct ConfigReadRequest {
    pub directory: PathBuf,
    pub includes: Vec<String>,
    pub alias: String,
    pub local_user: String,
}
pub(in super::super) struct PageConfigCapture {
    owner: Weak<ProfileReader>,
    registry: SessionRegistry,
    page: Arc<PageLaunchScope>,
    reader: ConfigDirectoryReader,
    snapshot: ConfigFileSnapshot,
    resolved: ResolvedConfig,
}
impl PageConfigCapture {
    pub(in super::super) fn resolved(&self) -> &ResolvedConfig {
        &self.resolved
    }
}
fn config_error(error: ConfigError) -> &'static str {
    match error {
        ConfigError::SourceChanged => "workspace.config_conflict",
        ConfigError::Unsupported | ConfigError::MissingInclude => "workspace.config_unsupported",
        ConfigError::Limit => "workspace.config_limit",
        ConfigError::InvalidTarget => "workspace.config_invalid",
        _ => "workspace.config_unavailable",
    }
}
impl ProfileReader {
    pub(in super::super) async fn capture_config(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        request: ConfigReadRequest,
    ) -> Result<Arc<PageConfigCapture>> {
        self.capture_config_inner(registry, page, request, || Ok(()))
            .await
    }
    async fn capture_config_inner(
        self: &Arc<Self>,
        registry: SessionRegistry,
        page: Arc<PageLaunchScope>,
        request: ConfigReadRequest,
        after_read: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<Arc<PageConfigCapture>> {
        self.check(&registry, &page)?;
        ConfigQuery::new(&request.alias, &request.local_user, None, None).map_err(config_error)?;
        let credit = self.reserve_list(&registry, &page).await?;
        let owner = self.clone();
        blocking(credit, move || {
            owner.check(&registry, &page)?;
            let reader = open_reader(&request.directory).map_err(config_error)?;
            let query = ConfigQuery::new(&request.alias, &request.local_user, None, None)
                .map_err(config_error)?;
            // Empty backend catalog selects bounded discovery; page callers cannot supply this catalog.
            let snapshot = if request.includes.is_empty() {
                reader.capture_discovered(&query)
            } else {
                let names: Vec<_> = request.includes.iter().map(String::as_str).collect();
                reader.capture(&names)
            }
            .map_err(config_error)?;
            let resolved = snapshot.resolve(&query).map_err(config_error)?;
            reader.confirm(&snapshot).map_err(config_error)?;
            after_read()?;
            owner.check(&registry, &page)?;
            Ok(Arc::new(PageConfigCapture {
                owner: Arc::downgrade(&owner),
                registry,
                page,
                reader,
                snapshot,
                resolved,
            }))
        })
        .await
    }
    pub(in super::super) async fn confirm_config(
        self: &Arc<Self>,
        capture: Arc<PageConfigCapture>,
    ) -> Result<()> {
        if !Weak::ptr_eq(&capture.owner, &Arc::downgrade(self)) {
            return Err("workspace.denied");
        }
        self.confirm_config_inner(capture, || Ok(())).await
    }
    async fn confirm_config_inner(
        self: &Arc<Self>,
        capture: Arc<PageConfigCapture>,
        entered: impl FnOnce() -> Result<()> + Send + 'static,
    ) -> Result<()> {
        if !Weak::ptr_eq(&capture.owner, &Arc::downgrade(self)) {
            return Err("workspace.denied");
        }
        self.check(&capture.registry, &capture.page)?;
        let credit = self.reserve_list(&capture.registry, &capture.page).await?;
        let owner = self.clone();
        blocking(credit, move || {
            owner.check(&capture.registry, &capture.page)?;
            entered()?;
            capture
                .reader
                .confirm(&capture.snapshot)
                .map_err(config_error)?;
            owner.check(&capture.registry, &capture.page)
        })
        .await
    }
}
#[path = "product_profiles_config_import.rs"]
mod imports;
pub(in super::super) use imports::{ConfigImportCapture, ImportControl};
#[cfg(all(test, unix))]
#[path = "product_profiles_config_tests.rs"]
mod tests;
