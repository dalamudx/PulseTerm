//! Only an explicitly captured joint intent may use this session adapter.
use super::{Guard, Result, session::SessionSink};
use crate::ssh_session_adapter::interactive::Broker;
use pulse_app::vault_owner::{
    AuthenticatedProtocolParts, PrivateKeyInteractiveSessionSink, PrivateKeySessionSink,
    PrivateKeySourceConfirmation, PrivateKeyTargetSink,
};
use pulse_core::TermSize;
use pulse_ssh::peer::PinnedTransport;
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
pub(super) fn requires_interactive(response: &russh::client::AuthResult) -> bool {
    matches!(response, russh::client::AuthResult::Failure { remaining_methods, partial_success: true }
        if remaining_methods.contains(&russh::MethodKind::KeyboardInteractive))
}

#[path = "ssh_private_key_interactive_forward.rs"]
pub(crate) mod forward;
pub(crate) struct JointSessionSink {
    inner: SessionSink,
}
impl JointSessionSink {
    pub(crate) fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        history_rows: usize,
        broker: Arc<Broker>,
    ) -> Result<Self> {
        let mut inner = SessionSink::new(transport, selected, size, history_rows)?;
        inner.require_interactive(broker);
        Ok(Self { inner })
    }
    pub(crate) fn observe_native_admission(&self) -> Guard {
        self.inner.observe_native_admission()
    }
    pub(crate) fn set_palette(&mut self, config: impl Into<pulse_app::TerminalStartConfig>) {
        self.inner.set_palette(config);
    }
    pub(crate) async fn new_route(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        history_rows: usize,
        broker: Arc<Broker>,
    ) -> std::result::Result<Self, bool> {
        let mut inner = SessionSink::new_route(transport, selected, size, history_rows).await?;
        inner.require_interactive(broker);
        Ok(Self { inner })
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.inner.attach_parents(parents);
    }
}
impl PrivateKeyInteractiveSessionSink for JointSessionSink {}
impl PrivateKeyTargetSink for JointSessionSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        key: &'a [u8],
        passphrase: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        self.inner
            .authenticate(username, key, passphrase, confirmation)
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.inner.close_and_drain()
    }
}
impl PrivateKeySessionSink for JointSessionSink {
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.inner.prepare_authenticated_terminal(guard)
    }
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        self.inner.take_authenticated_root_parts()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn joint_requires_partial_success_and_explicit_keyboard_interactive() {
        use russh::{MethodKind, client::AuthResult};
        let rejected = |partial_success, methods: Vec<MethodKind>| AuthResult::Failure {
            partial_success,
            remaining_methods: methods.as_slice().into(),
        };
        assert!(!requires_interactive(&AuthResult::Success));
        assert!(!requires_interactive(&rejected(
            false,
            vec![MethodKind::KeyboardInteractive]
        )));
        assert!(!requires_interactive(&rejected(
            true,
            vec![MethodKind::Password]
        )));
        assert!(!requires_interactive(&rejected(true, Vec::new())));
        assert!(requires_interactive(&rejected(
            true,
            vec![MethodKind::KeyboardInteractive]
        )));
    }
}
