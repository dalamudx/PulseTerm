//! Separate endpoint import action: original sources, page and database snapshot, no credentials.
use super::*;
use pulse_core::ssh_endpoint::SshEndpoint;
use pulse_store::ssh_hosts::{HostEdit, HostSnapshot};
use std::{
    sync::atomic::AtomicU8,
    time::{Duration, Instant},
};
pub(in super::super::super) struct ConfigImportCapture {
    source: Arc<PageConfigCapture>,
    directory: PathBuf,
    expected: HostSnapshot,
    endpoint: SshEndpoint,
}
impl ConfigImportCapture {
    pub(in super::super::super) fn endpoint(&self) -> &SshEndpoint {
        &self.endpoint
    }
    pub(in super::super::super) fn identity_file_count(&self) -> usize {
        self.source.resolved.identity_files().len()
    }
    pub(in super::super::super) fn page(&self) -> &Arc<PageLaunchScope> {
        &self.source.page
    }
}
// Prepared -> claimed -> revoked -> finished. Revocation is not native completion.
pub(in super::super::super) struct ImportControl {
    stage: AtomicU8,
    expires: Instant,
}
impl ImportControl {
    pub(in super::super::super) fn new() -> Arc<Self> {
        Arc::new(Self {
            stage: AtomicU8::new(0),
            expires: Instant::now() + Duration::from_secs(300),
        })
    }
    pub(in super::super::super) fn claim(&self) -> Result<()> {
        if Instant::now() >= self.expires {
            return Err("workspace.denied");
        }
        self.stage
            .compare_exchange(0, 1, Ordering::SeqCst, Ordering::SeqCst)
            .map(|_| ())
            .map_err(|_| "workspace.denied")
    }
    pub(in super::super::super) fn revoke(&self) {
        let _ = self
            .stage
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |stage| {
                Some(match stage {
                    0 => 3,
                    1 => 2,
                    value => value,
                })
            });
    }
    pub(in super::super::super) fn running(&self) -> bool {
        matches!(self.stage.load(Ordering::SeqCst), 1 | 2)
    }
    fn allowed(&self) -> bool {
        self.stage.load(Ordering::SeqCst) == 1 && Instant::now() < self.expires
    }
}
struct FinishImport(Arc<ImportControl>);
impl Drop for FinishImport {
    fn drop(&mut self) {
        self.0.stage.store(3, Ordering::SeqCst);
    }
}
struct CancelWait(Arc<ImportControl>);
impl Drop for CancelWait {
    fn drop(&mut self) {
        self.0.revoke();
    }
}
impl ProfileReader {
    pub(in super::super::super) async fn prepare_config_import(
        self: &Arc<Self>,
        source: Arc<PageConfigCapture>,
        directory: PathBuf,
        label: String,
    ) -> Result<Arc<ConfigImportCapture>> {
        if !Weak::ptr_eq(&source.owner, &Arc::downgrade(self)) {
            return Err("workspace.denied");
        }
        self.check(&source.registry, &source.page)?;
        let endpoint = SshEndpoint::new(
            &label,
            source.resolved.target().host(),
            source.resolved.target().port(),
            source.resolved.user(),
        )
        .map_err(|_| "workspace.invalid_host")?;
        self.confirm_config(source.clone()).await?;
        let expected = self
            .hosts(
                source.registry.clone(),
                source.page.clone(),
                directory.clone(),
            )
            .await?;
        self.confirm_config(source.clone()).await?;
        Ok(Arc::new(ConfigImportCapture {
            source,
            directory,
            expected,
            endpoint,
        }))
    }
    pub(in super::super::super) fn import_config(
        self: &Arc<Self>,
        capture: Arc<ConfigImportCapture>,
        control: Arc<ImportControl>,
    ) -> impl std::future::Future<Output = Result<HostSnapshot>> + Send + 'static {
        self.import_config_inner(capture, control, Arc::new(|| {}))
    }
    fn import_config_inner(
        self: &Arc<Self>,
        capture: Arc<ConfigImportCapture>,
        control: Arc<ImportControl>,
        before_check: Arc<dyn Fn() + Send + Sync>,
    ) -> impl std::future::Future<Output = Result<HostSnapshot>> + Send + 'static {
        let cancel_wait = CancelWait(control.clone());
        let completion = Arc::new(FinishImport(control.clone()));
        let reader = self.clone();
        async move {
            let _cancel_wait = cancel_wait;
            if !Weak::ptr_eq(&capture.source.owner, &Arc::downgrade(&reader)) || !control.allowed()
            {
                return Err("workspace.denied");
            }
            reader.check(&capture.source.registry, &capture.source.page)?;
            let credit = reader.reserve()?;
            tokio::spawn(async move {
                let store = reader
                    .prepare_store(
                        &capture.source.registry,
                        &capture.source.page,
                        capture.directory.clone(),
                        credit.clone(),
                    )
                    .await?;
                let failures = Arc::new(Mutex::new(None));
                let gate_failures = failures.clone();
                let gate_capture = capture.clone();
                let gate_reader = reader.clone();
                let gate = Arc::new(move || {
                    let _native_lifetime = (&credit, &completion);
                    before_check();
                    let source = &gate_capture.source;
                    let result = (|| {
                        if !control.allowed() {
                            return Err("workspace.denied");
                        }
                        gate_reader.check(&source.registry, &source.page)?;
                        source
                            .reader
                            .confirm(&source.snapshot)
                            .map_err(config_error)?;
                        if !control.allowed() {
                            return Err("workspace.denied");
                        }
                        gate_reader.check(&source.registry, &source.page)
                    })();
                    if let Err(error) = result
                        && let Ok(mut value) = gate_failures.lock()
                        && value.is_none()
                    {
                        *value = Some(error);
                    }
                    result.is_ok()
                });
                let result = store
                    .edit_ssh_hosts(
                        capture.expected.clone(),
                        HostEdit::Create(capture.endpoint.clone()),
                        gate,
                    )
                    .await;
                match result {
                    Ok(snapshot) => Ok(snapshot),
                    Err(pulse_store::StoreError::HostAuthorization) => Err(failures
                        .lock()
                        .ok()
                        .and_then(|v| *v)
                        .unwrap_or("workspace.denied")),
                    Err(pulse_store::StoreError::HostConflict) => Err("workspace.hosts_conflict"),
                    Err(pulse_store::StoreError::Busy) => Err("workspace.busy"),
                    Err(_) => Err("workspace.hosts_failed"),
                }
            })
            .await
            .map_err(|_| "workspace.hosts_failed")?
        }
    }
}
#[cfg(test)]
#[path = "product_profiles_config_import_tests.rs"]
mod tests;
