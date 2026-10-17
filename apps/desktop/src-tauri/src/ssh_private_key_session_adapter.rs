//! Trusted private-key session adapter; no page command or connection selection is implemented here.
use super::{Guard, Result, Sink};
use crate::ssh_terminal_io as terminal_io;
use pulse_app::vault_owner::{
    AuthenticatedProtocolParts, PrivateKeySessionSink, PrivateKeySourceConfirmation,
    PrivateKeyTargetSink, VaultOwnerError,
};
use pulse_core::TermSize;
use pulse_ssh::peer::PinnedTransport;
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use russh::{Channel, ChannelMsg, client};
use std::{future::Future, pin::Pin, sync::Arc};
use uuid::Uuid;

pub(crate) struct SessionSink {
    base: Sink,
    channel: Option<Channel<client::Msg>>,
    initial: Vec<Vec<u8>>,
    size: TermSize,
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
        size: TermSize,
        history_rows: usize,
    ) -> std::result::Result<Self, bool> {
        if size.columns() < 2
            || size.columns() > 160
            || size.rows() < 2
            || size.rows() > 60
            || history_rows > 10000
        {
            return Err(transport.close_and_join().await.is_ok());
        }
        Ok(Self {
            base: Sink::new_route(transport, selected).await?,
            channel: None,
            initial: Vec::new(),
            size,
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
        size: TermSize,
        history_rows: usize,
    ) -> Result<Self> {
        if size.columns() < 2
            || size.columns() > 160
            || size.rows() < 2
            || size.rows() > 60
            || history_rows > 10000
        {
            return Err(VaultOwnerError::Denied);
        }
        Ok(Self {
            base: Sink::new(transport, selected)?,
            channel: None,
            initial: Vec::new(),
            size,
            history_rows,
            palette: pulse_term::QueryPalette::default(),
            history_limit: 100_000,
            term_type: "xterm-256color",
            behavior: Default::default(),
            parents: None,
        })
    }
    pub(super) fn require_interactive(
        &mut self,
        broker: Arc<crate::ssh_session_adapter::interactive::Broker>,
    ) {
        self.base.interactive = Some(broker);
    }
    pub(crate) fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.parents = Some(parents);
    }
    pub(crate) fn observe_native_admission(&self) -> Guard {
        let weak = Arc::downgrade(&self.base.admission);
        Arc::new(move || weak.upgrade().is_some())
    }
    async fn success(&mut self, guard: &Guard) -> Result<()> {
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
impl PrivateKeyTargetSink for SessionSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        self.base.matches_target(source, host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.base.retain_native_authorization(guard)
    }
    fn authenticate<'a>(
        &'a mut self,
        user: &'a str,
        key: &'a [u8],
        pass: Option<&'a [u8]>,
        confirmation: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        self.base.authenticate(user, key, pass, confirmation)
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
        Box::pin(async move {
            // Release the receive queue before SDK join, including failed PTY/shell setup.
            self.channel.take();
            Box::pin(async move {
                let leaf = self.base.close_and_drain().await;
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
            .await
        })
    }
}
impl PrivateKeySessionSink for SessionSink {
    fn prepare_authenticated_terminal(
        &mut self,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + '_>> {
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
                    u32::from(self.size.columns()),
                    u32::from(self.size.rows()),
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
    fn take_authenticated_root_parts(&mut self) -> Result<AuthenticatedProtocolParts> {
        if self.channel.is_none()
            || self.base.transport.is_none()
            || self.base.completion.is_none()
            || self.base.decode.is_some()
        {
            return Err(VaultOwnerError::Denied);
        }
        let held = self
            .base
            .admission
            .guard
            .lock()
            .map_err(|_| VaultOwnerError::Failed)?
            .take();
        drop(held);
        // Complete all fallible checks before moving physical resources; root retains lifetime quota.
        let channel = self.channel.take().ok_or(VaultOwnerError::Denied)?;
        let transport = self.base.transport.take().ok_or(VaultOwnerError::Denied)?;
        let completion = self.base.completion.take().ok_or(VaultOwnerError::Denied)?;
        let parts = terminal_io::authenticated_parts(
            channel,
            transport,
            completion,
            self.base.peer.clone(),
            std::mem::take(&mut self.initial),
            terminal_io::TerminalConfig {
                size: self.size,
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
