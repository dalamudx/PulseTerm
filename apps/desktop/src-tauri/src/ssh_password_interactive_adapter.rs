//! Password borrowing ends before the explicit second-factor challenge is started.
use super::interactive::{Broker, authenticate_client};
use super::{Guard, Result, SessionSink};
use pulse_app::vault_owner::{
    AuthenticatedProtocolParts, PasswordInteractiveSessionSink, PasswordSessionSink,
    PasswordTargetSink, VaultOwnerError,
};
use pulse_core::TermSize;
use pulse_ssh::peer::PinnedTransport;
use pulse_store::{StateStore, ssh_target_source::SshTargetSource};
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;

#[path = "ssh_password_interactive_forward.rs"]
pub(crate) mod forward;
pub(crate) struct JointSessionSink {
    inner: SessionSink,
    broker: Arc<Broker>,
    store: StateStore,
    source: SshTargetSource,
    partial_user: Option<String>,
}
fn requires_interactive(response: &russh::client::AuthResult) -> bool {
    matches!(response, russh::client::AuthResult::Failure { remaining_methods, partial_success: true }
        if remaining_methods.contains(&russh::MethodKind::KeyboardInteractive))
}
impl JointSessionSink {
    pub(crate) fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        rows: usize,
        broker: Arc<Broker>,
        store: StateStore,
        source: SshTargetSource,
    ) -> Result<Self> {
        if source.is_route_bound()
            || !source.supports_password_selection(selected.0, selected.1)
            || !source.target_liveness()()
        {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self {
            inner: SessionSink::new(transport, selected, size, rows)?,
            broker,
            store,
            source,
            partial_user: None,
        })
    }
    pub(crate) async fn new_route(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        rows: usize,
        broker: Arc<Broker>,
        store: StateStore,
        source: SshTargetSource,
    ) -> std::result::Result<Self, bool> {
        if !source.is_route_bound()
            || !source.supports_password_selection(selected.0, selected.1)
            || !source.target_liveness()()
        {
            return Err(transport.close_and_join().await.is_ok());
        }
        Ok(Self {
            inner: SessionSink::new_route(transport, selected, size, rows).await?,
            broker,
            store,
            source,
            partial_user: None,
        })
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.inner.attach_parents(parents);
    }
    async fn complete_second_factor(&mut self, guard: Guard) -> Result<()> {
        let username = self.partial_user.take().ok_or(VaultOwnerError::Denied)?;
        let peer = self.inner.base.peer.clone();
        let transport = self
            .inner
            .base
            .transport
            .as_mut()
            .ok_or(VaultOwnerError::Denied)?;
        let store = self.store.clone();
        let source = self.source.clone();
        let original = guard.clone();
        let success = authenticate_client(
            transport,
            &peer,
            &self.broker,
            &username,
            guard,
            move || {
                let store = store.clone();
                let source = source.clone();
                let guard = original.clone();
                async move {
                    store
                        .validate_ssh_target_source(source, guard)
                        .await
                        .map_err(|_| VaultOwnerError::Denied)
                }
            },
        )
        .await?;
        if !success {
            return Err(VaultOwnerError::Denied);
        }
        Ok(())
    }
    pub(crate) fn set_palette(&mut self, config: pulse_app::TerminalStartConfig) {
        self.inner.set_palette(config);
    }
    pub(crate) fn observe_native_admission(&self) -> Guard {
        let weak = self.inner.observe_admission();
        Arc::new(move || weak.upgrade().is_some())
    }
}
impl PasswordInteractiveSessionSink for JointSessionSink {}
impl PasswordTargetSink for JointSessionSink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        source.is_route_bound() == self.source.is_route_bound()
            && source.is_route_leaf() == self.source.is_route_leaf()
            && source
                .forward_target()
                .map(|target| (target.host(), target.port()))
                == self
                    .source
                    .forward_target()
                    .map(|target| (target.host(), target.port()))
            && source.is_temporary() == self.source.is_temporary()
            && self.source.target_liveness()()
            && self.inner.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        username: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
            if self.partial_user.is_some() {
                return Err(VaultOwnerError::Denied);
            }
            let peer = &self.inner.base.peer;
            let transport = self
                .inner
                .base
                .transport
                .as_mut()
                .ok_or(VaultOwnerError::Denied)?;
            let response = transport
                .client(peer)
                .map_err(|_| VaultOwnerError::Denied)?
                .authenticate_password(username, password)
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            let partial = requires_interactive(&response);
            if partial {
                self.partial_user = Some(username.to_owned());
            }
            // Return to the supervisor so its zeroizing password loan is released before MFA.
            Ok(partial)
        })
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        self.inner.close_and_drain()
    }
}
impl PasswordSessionSink for JointSessionSink {
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            self.complete_second_factor(guard.clone()).await?;
            self.inner.prepare_authenticated_terminal(guard).await
        })
    }
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        self.inner.take_authenticated_root_parts()
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn password_joint_requires_actual_partial_success_and_mfa() {
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
        assert!(!requires_interactive(&rejected(true, vec![])));
        assert!(requires_interactive(&rejected(
            true,
            vec![MethodKind::KeyboardInteractive]
        )));
    }
}
