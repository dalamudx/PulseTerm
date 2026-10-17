//! Original-click metadata I/O, before any operation reservation or protocol work.
use super::super::PageLaunchScope;
use super::*;
use pulse_store::{StateStore, ssh_route_source::SshRouteIdentity};
use uuid::Uuid;

impl SavedSessionGesture {
    /// Point-in-time metadata admission only; never a peer, native-use or credential receipt.
    /// The actual SQL read rechecks this capture and caller I/O authority before publishing.
    pub fn permits_route_read(&self, page: &Arc<PageLaunchScope>) -> bool {
        Arc::ptr_eq(page, &self.capture.page)
            && Instant::now() < self.capture.expires
            && self.capture.current()
    }

    /// Read the complete route under the original page, epoch and deadline, then bind the result
    /// to the original leaf selection. `io` must retain caller I/O credit until the worker exits.
    /// This performs no dialing, peer approval, operation reservation or credential borrowing.
    pub async fn read_route(
        self,
        page: &Arc<PageLaunchScope>,
        store: &StateStore,
        destination: Uuid,
        identity: SshRouteIdentity,
        io: Authorize,
    ) -> Result<RoutedSessionGesture> {
        if !self.permits_route_read(page) {
            return Err(VaultOwnerError::Denied);
        }
        // A private key-free copy is used only inside metadata I/O authorization; it is never
        // returned as a second gesture or used as a credential-borrow/native-use predicate.
        let metadata = PasswordSessionGesture {
            owner: self.capture.owner.clone(),
            page: self.capture.page.clone(),
            selected: self.capture.selected,
            epoch: self.capture.epoch.clone(),
            original: self.capture.original.clone(),
            expires: self.capture.expires,
        };
        let gate: Authorize =
            Arc::new(move || io() && Instant::now() < metadata.expires && metadata.current());
        if !gate() {
            return Err(VaultOwnerError::Denied);
        }
        let source = store
            .load_ssh_route_source(destination, identity, gate.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !gate() {
            return Err(VaultOwnerError::Denied);
        }
        self.select_route(source)
    }
}
