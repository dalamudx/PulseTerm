//! Trusted upper-layer SSH terminal adapter; never constructs page authority or returns plaintext.
//! Compiled only by the opt-in native connection feature and isolated tests of this SAME source.
use crate::ssh_terminal_io as terminal_io;
use pulse_app::vault_owner::{
    AuthenticatedProtocolParts, PasswordSessionSink, PasswordTargetSink, VaultOwnerError,
};
use pulse_core::TermSize;
use pulse_ssh::peer::PinnedTransport;
use pulse_store::ssh_target_source::SshTargetSource;
use russh::{Channel, ChannelMsg, client};
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;
#[path = "ssh_password_adapter.rs"]
mod secret_target;
use secret_target::{Admission, Guard, Sink};
type Result<T> = std::result::Result<T, VaultOwnerError>;
#[path = "ssh_agent_session_adapter.rs"]
pub(crate) mod agent;
#[path = "ssh_interactive_session.rs"]
pub(crate) mod interactive;
#[path = "ssh_session_kinds.rs"]
mod kinds;
#[path = "ssh_password_interactive_adapter.rs"]
pub(crate) mod password_interactive;
#[path = "ssh_session_requests.rs"]
pub(crate) mod requests;
#[path = "ssh_route_driver.rs"]
pub(crate) mod route;
#[path = "ssh_session_start.rs"]
pub(crate) mod starter;
pub(crate) struct SessionSink {
    base: Sink,
    channel: Option<Channel<client::Msg>>,
    initial: Vec<Vec<u8>>,
    initial_size: TermSize,
    history_rows: usize,
    palette: pulse_term::QueryPalette,
    history_limit: usize,
    term_type: &'static str,
    behavior: pulse_store::profile_editor::ProfileAppearance,
    parents: Option<pulse_ssh::peer::RouteTransports>,
}
impl SessionSink {
    /// Fixed backend startup preference; does not grant authentication authority.
    pub(crate) fn set_palette(&mut self, config: impl Into<pulse_app::TerminalStartConfig>) {
        let config = config.into();
        self.palette = config.palette;
        self.history_limit = config.history_limit;
        self.term_type = config.term_type.as_str();
        self.behavior = config.behavior;
    }
    pub(crate) async fn new_route(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        initial_size: TermSize,
        history_rows: usize,
    ) -> std::result::Result<Self, bool> {
        if initial_size.columns() < 2
            || initial_size.columns() > 160
            || initial_size.rows() < 2
            || initial_size.rows() > 60
            || history_rows > 10000
        {
            return Err(transport.close_and_join().await.is_ok());
        }
        Ok(Self {
            base: Sink::new_route(transport, selected).await?,
            channel: None,
            initial: Vec::new(),
            initial_size,
            history_rows,
            palette: pulse_term::QueryPalette::default(),
            history_limit: 100_000,
            term_type: "xterm-256color",
            behavior: Default::default(),
            parents: None,
        })
    }
    pub(crate) fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        initial_size: TermSize,
        history_rows: usize,
    ) -> Result<Self> {
        if initial_size.columns() < 2
            || initial_size.columns() > 160
            || initial_size.rows() < 2
            || initial_size.rows() > 60
            || history_rows > 10000
        {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self {
            base: Sink::new(transport, selected)?,
            channel: None,
            initial: Vec::new(),
            initial_size,
            history_rows,
            palette: pulse_term::QueryPalette::default(),
            history_limit: 100_000,
            term_type: "xterm-256color",
            behavior: Default::default(),
            parents: None,
        })
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.parents = Some(parents);
    }
    pub(crate) fn observe_admission(&self) -> std::sync::Weak<Admission> {
        Arc::downgrade(&self.base.admission)
    }
    async fn success(&mut self, guard: &Guard) -> std::result::Result<(), VaultOwnerError> {
        loop {
            if !guard() || !self.base.peer.is_current() {
                return Err(VaultOwnerError::Denied);
            }
            match self
                .channel
                .as_mut()
                .ok_or(VaultOwnerError::Denied)?
                .wait()
                .await
            {
                Some(ChannelMsg::Success) => return Ok(()),
                Some(ChannelMsg::Data { data })
                    if self.initial.iter().map(Vec::len).sum::<usize>() + data.len() <= 8192 =>
                {
                    self.initial.push(data.to_vec())
                }
                _ => return Err(VaultOwnerError::Failed),
            }
        }
    }
}
impl PasswordTargetSink for SessionSink {
    fn matches_target(&self, source: &SshTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.base.matches_target(source, host, identity)
    }
    fn retain_native_authorization(
        &mut self,
        guard: Guard,
    ) -> std::result::Result<(), VaultOwnerError> {
        self.base.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        password: &'a str,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<bool, VaultOwnerError>> + Send + 'a>> {
        self.base.authenticate(user, password)
    }
    fn close_and_drain(
        &mut self,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            let leaf = async {
                // Release receive queues BEFORE SDK join (connection-wide receive HOL gate).
                self.channel.take();
                if let Some(transport) = self.base.transport.take() {
                    transport
                        .close_and_join()
                        .await
                        .map_err(|_| VaultOwnerError::Failed)?;
                }
                if let Some(completion) = self.base.completion.take() {
                    completion
                        .wait()
                        .await
                        .map_err(|_| VaultOwnerError::Failed)?;
                }
                let held = self
                    .base
                    .admission
                    .guard
                    .lock()
                    .map_err(|_| VaultOwnerError::Failed)?
                    .take();
                drop(held);
                Ok(())
            }
            .await;
            let parents = match self.parents.take() {
                Some(parents) => parents
                    .begin_close()
                    .wait()
                    .await
                    .map_err(|_| VaultOwnerError::Failed),
                None => Ok(()),
            };
            leaf.and(parents)
        })
    }
}
impl PasswordSessionSink for SessionSink {
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + '_>> {
        Box::pin(async move {
            if !guard() {
                return Err(VaultOwnerError::Denied);
            }
            self.channel = Some(
                self.base
                    .transport
                    .as_mut()
                    .ok_or(VaultOwnerError::Denied)?
                    .client(&self.base.peer)
                    .map_err(|_| VaultOwnerError::Denied)?
                    .channel_open_session()
                    .await
                    .map_err(|_| VaultOwnerError::Failed)?,
            );
            self.channel
                .as_ref()
                .ok_or(VaultOwnerError::Denied)?
                .request_pty(
                    true,
                    self.term_type,
                    u32::from(self.initial_size.columns()),
                    u32::from(self.initial_size.rows()),
                    0,
                    0,
                    &[],
                )
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            self.success(&guard).await?;
            self.channel
                .as_ref()
                .ok_or(VaultOwnerError::Denied)?
                .request_shell(true)
                .await
                .map_err(|_| VaultOwnerError::Failed)?;
            self.success(&guard).await
        })
    }
    fn take_authenticated_root_parts(
        &mut self,
    ) -> std::result::Result<AuthenticatedProtocolParts, VaultOwnerError> {
        let size = self.initial_size;
        if self.channel.is_none() || self.base.transport.is_none() || self.base.completion.is_none()
        {
            return Err(VaultOwnerError::Denied);
        }
        // Original credential admission ends after actual auth/setup replies. Persistent protocol
        // quota lives in StartedProtocolSession; underlying native admission remains until I/O exits.
        let held = self
            .base
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?
            .take();
        drop(held);
        // All fallible checks/locks complete before moving any native resource out of sink.
        let channel = self.channel.take().ok_or(VaultOwnerError::Denied)?;
        let transport = self.base.transport.take().ok_or(VaultOwnerError::Denied)?;
        let completion = self.base.completion.take().ok_or(VaultOwnerError::Denied)?;
        let peer = self.base.peer.clone();
        let initial = std::mem::take(&mut self.initial);
        let parts = terminal_io::authenticated_parts(
            channel,
            transport,
            completion,
            peer,
            initial,
            terminal_io::TerminalConfig {
                size,
                history_rows: self.history_rows,
                palette: self.palette.clone(),
                history_limit: self.history_limit,
            },
        );
        let parts = parts.with_appearance(self.behavior.clone());
        Ok(match self.parents.take() {
            Some(parents) => parts.with_native_tail(async move {
                parents.begin_close().wait().await.map_err(|_| {
                    pulse_app::protocol_session_owner::ProtocolSessionError::NativeUnknown
                })
            }),
            None => parts,
        })
    }
}
