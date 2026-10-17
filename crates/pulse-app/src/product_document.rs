//! Backend-only document gate for a future product workspace. Not a Tauri command.
//! The native adapter must verify the real WebView, local asset origin and owner;
//! after Finished it generates a random nonce and injects it into that document.
//! Neither WindowToken, nonce generation nor this controller is delegated to JS.
use crate::registry::{PageLaunchScope, RegistryError, SessionRegistry, WindowToken};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, MutexGuard},
};
use uuid::Uuid;

const MAX_DOCUMENT_WINDOWS: usize = 32;

enum Phase {
    Unloaded,
    Loading,
    Awaiting {
        nonce: Uuid,
        checkpoint: Arc<()>,
    },
    Ready {
        nonce: Uuid,
        scope: Arc<PageLaunchScope>,
        checkpoint: Arc<()>,
    },
    Invalid,
}

/// Native-only reference to one already-Finished document, not a page permission.
/// Confirmation may resolve it; navigation or another controller cannot upgrade it.
/// The native adapter must first verify its real window and local source.
pub struct PendingDocument {
    controller: Arc<()>,
    owner: WindowToken,
    checkpoint: Arc<()>,
}
/// Synchronous, bounded owner/nonce lifecycle; never hold this lock over I/O or an await.
/// Lock order: controller → registry. Navigation revokes page tickets and view bindings,
/// not sessions. It is separate from the finite two-refresh validation PageTracker.
pub struct ProductDocuments {
    registry: SessionRegistry,
    pages: Mutex<HashMap<WindowToken, Phase>>,
    identity: Arc<()>,
    changed: tokio::sync::Notify,
}

impl ProductDocuments {
    /// Create a controller for a separately owned product registry.
    pub fn new(registry: SessionRegistry) -> Self {
        Self {
            registry,
            pages: Mutex::new(HashMap::new()),
            identity: Arc::new(()),
            changed: tokio::sync::Notify::new(),
        }
    }

    fn lock(&self) -> Result<MutexGuard<'_, HashMap<WindowToken, Phase>>, RegistryError> {
        self.pages.lock().map_err(|_| RegistryError::Unavailable)
    }

    /// Install only a native-minted, open window; a duplicate cannot reset its page.
    pub fn install(&self, owner: WindowToken) -> Result<(), RegistryError> {
        let mut pages = self.lock()?;
        if pages.contains_key(&owner) {
            return Err(RegistryError::Forbidden);
        }
        if pages.len() >= MAX_DOCUMENT_WINDOWS {
            return Err(RegistryError::Busy);
        }
        self.registry.retire_window_views(owner)?;
        pages.insert(owner, Phase::Unloaded);
        Ok(())
    }

    /// Call at native PageLoad::Started, before any subsequent page request is admitted.
    /// An overlapping Started without a Finished is ambiguous and fails closed.
    pub fn started(&self, owner: WindowToken) -> Result<(), RegistryError> {
        let mut pages = self.lock()?;
        let phase = pages.get_mut(&owner).ok_or(RegistryError::Forbidden)?;
        let valid = !matches!(phase, Phase::Loading | Phase::Invalid);
        *phase = if valid {
            Phase::Loading
        } else {
            Phase::Invalid
        };
        self.changed.notify_waiters();
        // Even if an event is malformed, revoke old externally held scopes/tickets.
        self.registry.retire_window_views(owner)?;
        if valid {
            Ok(())
        } else {
            Err(RegistryError::Forbidden)
        }
    }

    /// Call only on native Finished. Generate the document nonce here, never accept
    /// an IPC-supplied value. A Finished without an unambiguous Started is rejected.
    /// The adapter injects the returned nonce into the currently rendered local page.
    pub fn finished(&self, owner: WindowToken) -> Result<Uuid, RegistryError> {
        let mut pages = self.lock()?;
        let phase = pages.get_mut(&owner).ok_or(RegistryError::Forbidden)?;
        if !matches!(phase, Phase::Loading) {
            *phase = Phase::Invalid;
            self.changed.notify_waiters();
            self.registry.retire_window_views(owner)?;
            return Err(RegistryError::Forbidden);
        }
        let nonce = Uuid::new_v4();
        *phase = Phase::Awaiting {
            nonce,
            checkpoint: Arc::new(()),
        };
        self.changed.notify_waiters();
        Ok(nonce)
    }

    /// After the current document confirms its injected nonce and the native adapter
    /// rechecks its owner/origin, activate that page in the same short lock. No I/O here.
    pub fn confirm(&self, owner: WindowToken, nonce: Uuid) -> Result<(), RegistryError> {
        let mut pages = self.lock()?;
        let phase = pages.get_mut(&owner).ok_or(RegistryError::Forbidden)?;
        let Phase::Awaiting {
            nonce: expected,
            checkpoint,
        } = phase
        else {
            return Err(RegistryError::Forbidden);
        };
        if *expected != nonce {
            return Err(RegistryError::Forbidden);
        }
        let checkpoint = Arc::clone(checkpoint);
        let scope = Arc::new(self.registry.activate_launch_page(owner)?);
        *phase = Phase::Ready {
            nonce,
            scope,
            checkpoint,
        };
        self.changed.notify_waiters();
        Ok(())
    }

    /// Instantaneous check for a current document. A returned scope is NOT a durable
    /// grant: after an async store read the launch path must reserve a current ticket.
    pub fn current(
        &self,
        owner: WindowToken,
        nonce: Uuid,
    ) -> Result<Arc<PageLaunchScope>, RegistryError> {
        let pages = self.lock()?;
        let Some(Phase::Ready {
            nonce: expected,
            scope,
            ..
        }) = pages.get(&owner)
        else {
            return Err(RegistryError::Forbidden);
        };
        if *expected != nonce {
            return Err(RegistryError::Forbidden);
        }
        self.registry.check_launch_page(scope)?;
        Ok(Arc::clone(scope))
    }

    /// Read the already-confirmed current page without accepting a client nonce. Only
    /// trusted native adapters holding the original WindowToken may use this; it is
    /// not an IPC command. The scope is revoked by navigation, so check again at
    /// the actual effect boundary after any await or main-thread handoff.
    pub fn current_native(
        &self,
        owner: WindowToken,
    ) -> Result<Arc<PageLaunchScope>, RegistryError> {
        self.current_native_document(owner).map(|(_, scope)| scope)
    }

    /// Atomically read the confirmed native document nonce and its original scope.
    /// This read-only tuple is not authority: adapters still validate actual source
    /// and recheck the original scope at the effect boundary. Pending pages fail.
    pub fn current_native_document(
        &self,
        owner: WindowToken,
    ) -> Result<(Uuid, Arc<PageLaunchScope>), RegistryError> {
        let pages = self.lock()?;
        let Some(Phase::Ready { nonce, scope, .. }) = pages.get(&owner) else {
            return Err(RegistryError::Forbidden);
        };
        self.registry.check_launch_page(scope)?;
        Ok((*nonce, Arc::clone(scope)))
    }

    /// Capture a specific Finished document before or after its confirmation.
    /// Unloaded/Loading cannot be captured; this never activates a launch page.
    pub fn capture_pending_native(
        &self,
        owner: WindowToken,
    ) -> Result<PendingDocument, RegistryError> {
        let pages = self.lock()?;
        let checkpoint = match pages.get(&owner) {
            Some(Phase::Awaiting { checkpoint, .. } | Phase::Ready { checkpoint, .. }) => {
                checkpoint
            }
            _ => return Err(RegistryError::Forbidden),
        };
        Ok(PendingDocument {
            controller: Arc::clone(&self.identity),
            owner,
            checkpoint: Arc::clone(checkpoint),
        })
    }

    /// None means the original document is still unconfirmed, never permission.
    /// Checkpoint identity is independent of nonce bytes and survives only its
    /// own Awaiting→Ready transition. The returned scope still needs admission.
    pub fn resolve_pending_native(
        &self,
        captured: &PendingDocument,
    ) -> Result<Option<Arc<PageLaunchScope>>, RegistryError> {
        if !Arc::ptr_eq(&self.identity, &captured.controller) {
            return Err(RegistryError::Forbidden);
        }
        let pages = self.lock()?;
        match pages.get(&captured.owner) {
            Some(Phase::Awaiting { checkpoint, .. })
                if Arc::ptr_eq(checkpoint, &captured.checkpoint) =>
            {
                Ok(None)
            }
            Some(Phase::Ready {
                checkpoint, scope, ..
            }) if Arc::ptr_eq(checkpoint, &captured.checkpoint) => {
                self.registry.check_launch_page(scope)?;
                Ok(Some(Arc::clone(scope)))
            }
            _ => Err(RegistryError::Forbidden),
        }
    }

    /// Bounded, cancellable metadata-only wait, without holding any I/O credit.
    /// The backend chooses a budget up to 8s. Native startup and human approval
    /// have separate existing deadlines. A late/new document cannot satisfy it.
    pub async fn wait_pending_native(
        &self,
        captured: &PendingDocument,
        budget: std::time::Duration,
    ) -> Result<Arc<PageLaunchScope>, RegistryError> {
        if budget.is_zero() || budget > std::time::Duration::from_secs(8) {
            return Err(RegistryError::Busy);
        }
        tokio::time::timeout(budget, async {
            loop {
                let changed = self.changed.notified();
                tokio::pin!(changed);
                changed.as_mut().enable();
                if let Some(scope) = self.resolve_pending_native(captured)? {
                    return Ok(scope);
                }
                changed.await;
            }
        })
        .await
        .map_err(|_| RegistryError::Busy)?
    }

    /// Drop the page without closing owned sessions. The native window owner must
    /// separately perform its normal close/transfer transaction in SessionRegistry.
    pub fn remove(&self, owner: WindowToken) -> Result<(), RegistryError> {
        let mut pages = self.lock()?;
        if pages.remove(&owner).is_none() {
            return Err(RegistryError::Forbidden);
        }
        self.changed.notify_waiters();
        self.registry.retire_window_views(owner)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "product_document_pending_tests.rs"]
mod pending_tests;
#[cfg(test)]
#[path = "product_document_tests.rs"]
mod tests;
