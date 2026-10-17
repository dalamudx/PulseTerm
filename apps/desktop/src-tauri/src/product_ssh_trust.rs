//! Public key inspection/first-use approval only. No authentication or terminal session creation.
#[cfg(all(test, feature = "workspace-ssh", unix))]
#[path = "product_ssh_trust_tests.rs"]
mod tests;
use super::*;
#[cfg(all(feature = "workspace-ssh", unix))]
use pulse_app::registry::trust_action::{TrustActionOperation, TrustActionRevocation};
use pulse_app::registry::{PageLaunchScope, trust_action::PageTrustActions};
#[cfg(all(feature = "workspace-ssh", unix))]
use pulse_ssh::{
    managed_store::{FirstPinApproval, FirstPinObservation},
    peer::{ObservedHostKey, OfferedKeyStatus},
};
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct TicketDto {
    version: u8,
    token: String,
}
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(super) struct FingerprintDto {
    version: u8,
    token: Option<String>,
    label: String,
    host: String,
    port: u16,
    algorithm: String,
    fingerprint: String,
    status: &'static str,
    source: &'static str,
}
#[derive(serde::Serialize)]
pub(super) struct RevisionDto {
    version: u8,
    revision: String,
}
pub(super) struct TrustBank {
    actions: PageTrustActions,
    #[cfg(all(feature = "workspace-ssh", unix))]
    pending: Mutex<Option<Entry>>,
}
#[cfg(all(feature = "workspace-ssh", unix))]
struct Ready {
    action: TrustActionOperation,
    source: pulse_store::ssh_target_source::SshTargetSource,
    endpoint: pulse_core::ssh_endpoint::SshEndpoint,
}
#[cfg(all(feature = "workspace-ssh", unix))]
struct Approval {
    action: TrustActionOperation,
    prompt: Box<FirstPinApproval>,
}
#[cfg(all(feature = "workspace-ssh", unix))]
enum Stage {
    Ready(Box<Ready>),
    Observing(TrustActionRevocation),
    Confirming(TrustActionRevocation),
    Approval(Box<Approval>),
}
#[cfg(all(feature = "workspace-ssh", unix))]
struct Entry {
    page: Arc<PageLaunchScope>,
    token: Uuid,
    stage: Stage,
    ready_expires: tokio::time::Instant,
}
impl TrustBank {
    pub(super) fn new(registry: SessionRegistry, window: WindowToken) -> Self {
        Self {
            actions: PageTrustActions::new(registry, window),
            #[cfg(all(feature = "workspace-ssh", unix))]
            pending: Mutex::new(None),
        }
    }
    pub(super) fn retire(&self) {
        self.actions.retire();
        #[cfg(all(feature = "workspace-ssh", unix))]
        {
            let previous = self
                .pending
                .lock()
                .unwrap_or_else(|p| p.into_inner())
                .take();
            drop(previous);
        }
    }
    pub(super) fn close(&self) {
        self.actions.close();
        self.retire();
    }
    pub(super) async fn shutdown(&self) -> Result<()> {
        self.close();
        self.actions
            .wait_idle()
            .await
            .map_err(|_| "workspace.ssh_shutdown_failed")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn remove(&self, token: Uuid, ready_only: bool) {
        let previous = {
            let mut guard = self.pending.lock().unwrap_or_else(|p| p.into_inner());
            if guard.as_ref().is_some_and(|entry| {
                entry.token == token && (!ready_only || matches!(entry.stage, Stage::Ready(_)))
            }) {
                guard.take()
            } else {
                None
            }
        };
        if let Some(Entry {
            stage: Stage::Observing(revoke) | Stage::Confirming(revoke),
            ..
        }) = &previous
        {
            revoke.cancel();
        }
        drop(previous);
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn publish(self: &Arc<Self>, page: Arc<PageLaunchScope>, ready: Ready) -> Result<TicketDto> {
        if !ready.action.is_current() {
            return Err("workspace.denied");
        }
        let cancel = ready.action.cancellation();
        let token = Uuid::new_v4();
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_secs(30);
        {
            let mut guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
            if guard.is_some() || !ready.action.is_current() {
                return Err("workspace.denied");
            }
            *guard = Some(Entry {
                page,
                token,
                stage: Stage::Ready(Box::new(ready)),
                ready_expires: deadline,
            });
        }
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            cancel.wait().await;
            if let Some(bank) = weak.upgrade() {
                bank.remove(token, false);
            }
        });
        let weak = Arc::downgrade(self);
        tokio::spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(bank) = weak.upgrade() {
                bank.remove(token, true);
            }
        });
        Ok(TicketDto {
            version: 1,
            token: token.to_string(),
        })
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn observe(&self, page: &Arc<PageLaunchScope>, token: Uuid) -> Result<Box<Ready>> {
        let mut guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
        let entry = guard.as_mut().ok_or("workspace.denied")?;
        if entry.token != token
            || !Arc::ptr_eq(&entry.page, page)
            || tokio::time::Instant::now() >= entry.ready_expires
        {
            return Err("workspace.denied");
        }
        let Stage::Ready(ready) = &entry.stage else {
            return Err("workspace.denied");
        };
        if !ready.action.is_current() {
            return Err("workspace.denied");
        }
        let revoke = ready.action.revocation();
        match std::mem::replace(&mut entry.stage, Stage::Observing(revoke)) {
            Stage::Ready(ready) => Ok(ready),
            _ => Err("workspace.denied"),
        }
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn approve_ready(
        &self,
        page: &Arc<PageLaunchScope>,
        token: Uuid,
        approval: Approval,
    ) -> Result<()> {
        let previous = {
            let mut guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
            let entry = guard.as_mut().ok_or("workspace.denied")?;
            if entry.token != token
                || !Arc::ptr_eq(&entry.page, page)
                || !matches!(entry.stage, Stage::Observing(_))
                || !approval.action.is_current()
            {
                return Err("workspace.denied");
            }
            std::mem::replace(&mut entry.stage, Stage::Approval(Box::new(approval)))
        };
        drop(previous);
        Ok(())
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn take_approval(&self, page: &Arc<PageLaunchScope>, token: Uuid) -> Result<Box<Approval>> {
        let previous = {
            let mut guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
            let entry = guard.as_mut().ok_or("workspace.denied")?;
            let Stage::Approval(approval) = &entry.stage else {
                return Err("workspace.denied");
            };
            if entry.token != token
                || !Arc::ptr_eq(&entry.page, page)
                || !approval.action.is_current()
            {
                return Err("workspace.denied");
            }
            let revoke = approval.action.revocation();
            std::mem::replace(&mut entry.stage, Stage::Confirming(revoke))
        };
        match previous {
            Stage::Approval(approval) => Ok(approval),
            _ => Err("workspace.denied"),
        }
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn complete(&self, token: Uuid) -> Result<()> {
        let previous = {
            let mut guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
            if guard
                .as_ref()
                .is_some_and(|e| e.token == token && matches!(e.stage, Stage::Confirming(_)))
            {
                guard.take()
            } else {
                return Err("workspace.denied");
            }
        };
        drop(previous);
        Ok(())
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    fn cancel(&self, page: &Arc<PageLaunchScope>, token: Uuid) -> Result<()> {
        {
            let guard = self.pending.lock().map_err(|_| "workspace.ssh_failed")?;
            let entry = guard.as_ref().ok_or("workspace.denied")?;
            if entry.token != token || !Arc::ptr_eq(&entry.page, page) {
                return Err("workspace.denied");
            }
        }
        self.remove(token, false);
        Ok(())
    }
}
#[cfg(all(feature = "workspace-ssh", unix))]
struct CancelOnDrop {
    revoke: TrustActionRevocation,
    armed: bool,
}
#[cfg(all(feature = "workspace-ssh", unix))]
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.revoke.cancel();
        }
    }
}
fn checked(window: &WebviewWindow, state: &Workspace, nonce: &str) -> Result<Arc<PageLaunchScope>> {
    if window.label() != state.native_label()? {
        return Err("workspace.denied");
    }
    let url = window.url().map_err(|_| "workspace.denied")?;
    current_document(&state.0.documents, state.0.owner, &url, nonce).map(|(_, page)| page)
}
#[cfg(all(feature = "workspace-ssh", unix))]
fn identity(text: &str, version: usize) -> Result<Uuid> {
    let id = Uuid::parse_str(text).map_err(|_| "workspace.denied")?;
    if id.get_version_num() != version
        || id.get_variant() != uuid::Variant::RFC4122
        || id.to_string() != text
    {
        return Err("workspace.denied");
    }
    Ok(id)
}
#[cfg(all(feature = "workspace-ssh", unix))]
fn projection(
    key: &ObservedHostKey,
    endpoint: &pulse_core::ssh_endpoint::SshEndpoint,
    token: Option<Uuid>,
) -> FingerprintDto {
    FingerprintDto {
        version: 1,
        token: token.map(|t| t.to_string()),
        label: endpoint.label().into(),
        host: endpoint.target().host().into(),
        port: endpoint.target().port(),
        algorithm: key.public_key().algorithm().to_string(),
        fingerprint: key
            .public_key()
            .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            .to_string(),
        status: match key.status() {
            OfferedKeyStatus::Unknown => "unknown",
            OfferedKeyStatus::Changed => "changed",
            OfferedKeyStatus::AlreadyPinned => "already_pinned",
        },
        source: "application_only",
    }
}
#[cfg(all(feature = "workspace-ssh", unix))]
async fn dial(
    endpoint: &pulse_core::ssh_endpoint::SshEndpoint,
    native: Arc<dyn Fn() -> bool + Send + Sync>,
) -> Result<tokio::net::TcpStream> {
    use std::net::ToSocketAddrs;
    let host = endpoint.target().host().to_owned();
    let port = endpoint.target().port();
    let resolving = native.clone();
    let lookup = tokio::task::spawn_blocking(move || {
        let _actual_dns_reservation = &resolving;
        if !resolving() {
            return Err("workspace.denied");
        }
        let addresses = (host.as_str(), port)
            .to_socket_addrs()
            .map_err(|_| "workspace.ssh_connect_failed")?
            .take(65)
            .collect::<Vec<_>>();
        if addresses.is_empty() || addresses.len() > 64 || !resolving() {
            return Err("workspace.ssh_connect_failed");
        }
        Ok(addresses)
    });
    tokio::time::timeout(std::time::Duration::from_secs(5), async move {
        let addresses = lookup.await.map_err(|_| "workspace.ssh_connect_failed")??;
        for address in addresses {
            if !native() {
                return Err("workspace.denied");
            }
            if let Ok(stream) = tokio::net::TcpStream::connect(address).await {
                if native() {
                    return Ok(stream);
                }
                return Err("workspace.denied");
            }
        }
        Err("workspace.ssh_connect_failed")
    })
    .await
    .map_err(|_| "workspace.ssh_connect_timeout")?
}
#[tauri::command]
pub(super) async fn workspace_ssh_fingerprint_prepare(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    host: String,
) -> Result<TicketDto> {
    let page = checked(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, host);
        Err("workspace.ssh_unavailable")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let action = state
            .0
            .ssh_trust
            .actions
            .begin(page.clone(), identity(&host, 7)?)
            .map_err(|_| "workspace.ssh_busy")?;
        let mut cancel = CancelOnDrop {
            revoke: action.revocation(),
            armed: true,
        };
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let (_, source) = state
            .0
            .profiles
            .ssh_trust_source(
                state.0.registry.clone(),
                page.clone(),
                directory,
                action.authorization(),
                state
                    .0
                    .quick_targets
                    .resolve(&state.0.registry, &page, action.host())?,
            )
            .await?;
        if !source.hosts().supports_direct_ssh(action.host()) {
            return Err("workspace.ssh_route_unavailable");
        }
        let endpoint = source
            .hosts()
            .records()
            .iter()
            .find(|h| h.id() == action.host())
            .ok_or("workspace.ssh_source_failed")?
            .endpoint()
            .clone();
        let current = checked(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) {
            return Err("workspace.denied");
        }
        let dto = state.0.ssh_trust.publish(
            page,
            Ready {
                action,
                source,
                endpoint,
            },
        )?;
        cancel.armed = false;
        eprintln!("workspace.ssh_fingerprint_prepared");
        Ok(dto)
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_fingerprint_observe(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<FingerprintDto> {
    let page = checked(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_unavailable")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let token = identity(&token, 4)?;
        let Ready {
            action,
            source,
            endpoint,
        } = *state.0.ssh_trust.observe(&page, token)?;
        let mut revoke = CancelOnDrop {
            revoke: action.revocation(),
            armed: true,
        };
        let cancel = action.cancellation();
        let native = action.authorization();
        let inspection = async {
            let stream = dial(&endpoint, native.clone()).await?;
            FirstPinApproval::inspect(
                source,
                action.host(),
                Some(vec![]),
                Arc::new(russh::client::Config::default()),
                native,
                stream,
            )
            .await
            .map_err(|_| "workspace.ssh_inspection_failed")
        };
        let outcome =
            tokio::select! {_=cancel.wait()=>Err("workspace.denied"),outcome=inspection=>outcome}?;
        let current = checked(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) || !action.is_current() {
            return Err("workspace.denied");
        }
        let dto = match outcome {
            FirstPinObservation::FirstUse(prompt) => {
                let dto = projection(prompt.candidate(), &endpoint, Some(token));
                state
                    .0
                    .ssh_trust
                    .approve_ready(&page, token, Approval { action, prompt })?;
                dto
            }
            FirstPinObservation::Blocked(key) => {
                let dto = projection(&key, &endpoint, None);
                state.0.ssh_trust.remove(token, false);
                dto
            }
        };
        revoke.armed = false;
        eprintln!("workspace.ssh_fingerprint_observed.{}", dto.status);
        Ok(dto)
    }
}
#[tauri::command]
pub(super) async fn workspace_ssh_fingerprint_confirm(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<RevisionDto> {
    let page = checked(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_unavailable")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        let Approval { action, prompt } = *state
            .0
            .ssh_trust
            .take_approval(&page, identity(&token, 4)?)?;
        let _cancel = CancelOnDrop {
            revoke: action.revocation(),
            armed: true,
        };
        let directory = window
            .app_handle()
            .path()
            .app_data_dir()
            .map_err(|_| "workspace.unavailable")?;
        let revision = state
            .0
            .profiles
            .ssh_pin_confirm(
                state.0.registry.clone(),
                page.clone(),
                directory,
                prompt,
                action.authorization(),
            )
            .await?;
        let current = checked(&window, &state, &nonce)?;
        if !Arc::ptr_eq(&current, &page) || !action.is_current() {
            return Err("workspace.denied");
        }
        state.0.ssh_trust.complete(identity(&token, 4)?)?;
        eprintln!("workspace.ssh_fingerprint_confirmed");
        Ok(RevisionDto {
            version: 1,
            revision: revision.to_string(),
        })
    }
}
#[tauri::command]
pub(super) fn workspace_ssh_fingerprint_cancel(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
    token: String,
) -> Result<()> {
    let page = checked(&window, &state, &nonce)?;
    #[cfg(not(all(feature = "workspace-ssh", unix)))]
    {
        let _ = (page, token);
        Err("workspace.ssh_unavailable")
    }
    #[cfg(all(feature = "workspace-ssh", unix))]
    {
        state.0.ssh_trust.cancel(&page, identity(&token, 4)?)?;
        eprintln!("workspace.ssh_fingerprint_cancelled");
        Ok(())
    }
}
