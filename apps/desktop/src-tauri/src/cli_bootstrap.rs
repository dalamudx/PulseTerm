//! Bounded native bootstrap metadata. Claiming is not Capability admission.
//! Supports read-only focus/profiles metadata; no native session or key is created.
use crate::cli_startup::{Error, StartupAction};
use pulse_app::{
    product_document::{PendingDocument, ProductDocuments},
    registry::{PageLaunchScope, WindowToken},
};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};
use tokio::sync::Notify;

const MAX_CLAIMS: usize = 8;
enum SourcePhase {
    Waiting { started: bool },
    Bound(Arc<PendingDocument>),
    Revoked,
}
struct Source {
    phase: Mutex<SourcePhase>,
    changed: Notify,
}
impl Source {
    fn new(phase: SourcePhase) -> Self {
        Self {
            phase: Mutex::new(phase),
            changed: Notify::new(),
        }
    }
    async fn document(&self) -> Result<Arc<PendingDocument>, Error> {
        loop {
            let changed = self.changed.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            let captured = {
                let phase = self.phase.lock().map_err(|_| Error)?;
                match &*phase {
                    SourcePhase::Waiting { .. } => None,
                    SourcePhase::Bound(document) => Some(document.clone()),
                    SourcePhase::Revoked => return Err(Error),
                }
            };
            if let Some(captured) = captured {
                return Ok(captured);
            }
            changed.await;
        }
    }
}
/// Immutable, once-claimed metadata and its original document reference.
/// It has no handler. The adapter must recheck cancellation and perform admission.
pub struct Claim {
    action: StartupAction,
    source: Arc<Source>,
}
impl Claim {
    /// Wait at most 8s in total for the original first-document/confirmation pair.
    /// This does not confirm a document or execute the stored action.
    pub async fn original_scope(
        &self,
        documents: &ProductDocuments,
    ) -> Result<Arc<PageLaunchScope>, Error> {
        tokio::time::timeout(Duration::from_secs(8), async {
            let document = self.source.document().await?;
            documents
                .wait_pending_native(&document, Duration::from_secs(8))
                .await
                .map_err(|_| Error)
        })
        .await
        .map_err(|_| Error)?
    }
    /// The immutable original action; no client override is accepted during claim.
    pub fn action(&self) -> &StartupAction {
        &self.action
    }
}
/// At most eight startup ids per app lifetime, including consumed tombstones.
/// Normal authenticated warm actions do not use this bank. It holds no I/O quota.
pub struct Bank {
    entries: Mutex<HashMap<String, Option<Claim>>>,
    first: Arc<Source>,
}
impl Default for Bank {
    fn default() -> Self {
        Self {
            entries: Mutex::new(HashMap::new()),
            first: Arc::new(Source::new(SourcePhase::Waiting { started: false })),
        }
    }
}
impl Bank {
    fn insert(&self, action: StartupAction, source: Arc<Source>) -> Result<(), Error> {
        if !action.supports_cold_transport() {
            return Err(Error);
        }
        let mut entries = self.entries.lock().map_err(|_| Error)?;
        if entries.len() >= MAX_CLAIMS || entries.contains_key(action.request_id()) {
            return Err(Error);
        }
        entries.insert(
            action.request_id().to_owned(),
            Some(Claim { action, source }),
        );
        Ok(())
    }
    /// For a backend-selected first window before its first Finished event only.
    pub fn enqueue_first(&self, action: StartupAction) -> Result<(), Error> {
        let phase = self.first.phase.lock().map_err(|_| Error)?;
        if !matches!(*phase, SourcePhase::Waiting { .. }) {
            return Err(Error);
        }
        self.insert(action, self.first.clone())
    }
    /// For forwarding to an existing actual local window and its specific document.
    pub fn enqueue_current(
        &self,
        action: StartupAction,
        documents: &ProductDocuments,
        owner: WindowToken,
    ) -> Result<(), Error> {
        let document = documents.capture_pending_native(owner).map_err(|_| Error)?;
        self.insert(
            action,
            Arc::new(Source::new(SourcePhase::Bound(Arc::new(document)))),
        )
    }
    /// Native Started revokes an initial-source wait on any subsequent navigation.
    pub fn started(&self) -> Result<(), Error> {
        let mut phase = self.first.phase.lock().map_err(|_| Error)?;
        *phase = match *phase {
            SourcePhase::Waiting { started: false } => SourcePhase::Waiting { started: true },
            _ => SourcePhase::Revoked,
        };
        self.first.changed.notify_waiters();
        Ok(())
    }
    /// Only the first native Finished binds the initial wait. Never rebind a retired id.
    pub fn finished(&self, documents: &ProductDocuments, owner: WindowToken) -> Result<(), Error> {
        let mut phase = self.first.phase.lock().map_err(|_| Error)?;
        if matches!(*phase, SourcePhase::Waiting { started: true }) {
            let document = documents.capture_pending_native(owner).map_err(|_| Error)?;
            *phase = SourcePhase::Bound(Arc::new(document));
            self.first.changed.notify_waiters();
        }
        Ok(())
    }
    /// Synchronous revocation on native close/exit; ordinary page scopes remain separate.
    pub fn close(&self) -> Result<(), Error> {
        *self.first.phase.lock().map_err(|_| Error)? = SourcePhase::Revoked;
        self.first.changed.notify_waiters();
        Ok(())
    }
    /// An authenticated transport may consume an id exactly once. Correlation is not auth.
    pub fn claim(&self, id: &str) -> Result<Claim, Error> {
        self.entries
            .lock()
            .map_err(|_| Error)?
            .get_mut(id)
            .and_then(Option::take)
            .ok_or(Error)
    }
}
#[cfg(test)]
#[path = "cli_bootstrap_tests.rs"]
mod tests;
