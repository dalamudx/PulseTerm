//! Bounded original-window challenge broker. Neither answers nor native handles are serialized.
use super::{Guard, SessionSink as TerminalSink};
use pulse_app::vault_owner::{
    AuthenticatedProtocolParts, InteractiveSessionSink, PasswordSessionSink, PasswordTargetSink,
    VaultOwnerError,
};
use pulse_core::TermSize;
use pulse_ssh::peer::PinnedTransport;
use pulse_store::{StateStore, ssh_key_target_source::SshKeyTargetSource};
use std::{
    future::Future,
    pin::Pin,
    sync::{Arc, Mutex, Weak},
};
use tokio::sync::oneshot;
use uuid::Uuid;
use zeroize::Zeroizing;
type Result<T> = std::result::Result<T, VaultOwnerError>;
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct Prompt {
    version: u8,
    round: String,
    name: String,
    instructions: String,
    prompts: Vec<Question>,
}
#[derive(serde::Serialize)]
pub(crate) struct Question {
    text: String,
    echo: bool,
}
struct Pending {
    round: Uuid,
    count: usize,
    guard: Guard,
    sender: oneshot::Sender<Zeroizing<Vec<String>>>,
}
pub(crate) struct Broker {
    output: Box<dyn Fn(serde_json::Value) -> Result<()> + Send + Sync>,
    pending: Mutex<Option<Pending>>,
}
struct Round {
    broker: Weak<Broker>,
    id: Uuid,
}
impl Drop for Round {
    fn drop(&mut self) {
        if let Some(broker) = self.broker.upgrade() {
            let mut pending = broker.pending.lock().unwrap_or_else(|p| p.into_inner());
            if pending.as_ref().is_some_and(|p| p.round == self.id) {
                pending.take();
            }
        }
    }
}
impl Broker {
    pub fn new(
        output: impl Fn(serde_json::Value) -> Result<()> + Send + Sync + 'static,
    ) -> Arc<Self> {
        Arc::new(Self {
            output: Box::new(output),
            pending: Mutex::new(None),
        })
    }
    /// Caller MUST have checked the exact original page and single request token first.
    pub fn answer(&self, round: Uuid, answers: Zeroizing<Vec<String>>) -> Result<()> {
        if answers.len() > 8
            || answers.iter().any(|a| a.len() > 4096 || a.contains('\0'))
            || answers.iter().map(String::len).sum::<usize>() > 16384
        {
            return Err(VaultOwnerError::Denied);
        }
        let mut pending = self.pending.lock().map_err(|_| VaultOwnerError::Failed)?;
        let p = pending.as_ref().ok_or(VaultOwnerError::Denied)?;
        if p.round != round || p.count != answers.len() || !(p.guard)() {
            return Err(VaultOwnerError::Denied);
        }
        let p = pending.take().ok_or(VaultOwnerError::Denied)?;
        p.sender.send(answers).map_err(|_| VaultOwnerError::Denied)
    }
    async fn ask(
        self: &Arc<Self>,
        name: String,
        instructions: String,
        prompts: Vec<russh::client::Prompt>,
        guard: Guard,
    ) -> Result<Zeroizing<Vec<String>>> {
        if !guard()
            || name.len() > 1024
            || instructions.len() > 4096
            || prompts.len() > 8
            || prompts.iter().any(|p| p.prompt.len() > 1024)
            || prompts.iter().map(|p| p.prompt.len()).sum::<usize>() > 8192
        {
            return Err(VaultOwnerError::Denied);
        }
        let id = Uuid::new_v4();
        let (sender, receiver) = oneshot::channel();
        {
            let mut p = self.pending.lock().map_err(|_| VaultOwnerError::Failed)?;
            if p.is_some() {
                return Err(VaultOwnerError::Denied);
            }
            *p = Some(Pending {
                round: id,
                count: prompts.len(),
                guard: guard.clone(),
                sender,
            });
        }
        let _round = Round {
            broker: Arc::downgrade(self),
            id,
        };
        let prompt = Prompt {
            version: 1,
            round: id.to_string(),
            name,
            instructions,
            prompts: prompts
                .into_iter()
                .map(|p| Question {
                    text: p.prompt,
                    echo: p.echo,
                })
                .collect(),
        };
        (self.output)(serde_json::to_value(prompt).map_err(|_| VaultOwnerError::Denied)?)?;
        let answers = receiver.await.map_err(|_| VaultOwnerError::Denied)?;
        if !guard() {
            return Err(VaultOwnerError::Denied);
        }
        Ok(answers)
    }
}
pub(crate) struct SessionSink {
    inner: TerminalSink,
    broker: Arc<Broker>,
    store: StateStore,
    source: SshKeyTargetSource,
}
#[path = "ssh_interactive_forward.rs"]
pub(crate) mod forward;
impl SessionSink {
    pub async fn new_route(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        rows: usize,
        broker: Arc<Broker>,
        store: StateStore,
        source: SshKeyTargetSource,
    ) -> std::result::Result<Self, bool> {
        Ok(Self {
            inner: TerminalSink::new_route(transport, selected, size, rows).await?,
            broker,
            store,
            source,
        })
    }
    pub fn attach_parents(&mut self, parents: pulse_ssh::peer::RouteTransports) {
        self.inner.attach_parents(parents);
    }
    pub fn new(
        transport: PinnedTransport,
        selected: (Uuid, Uuid),
        size: TermSize,
        rows: usize,
        broker: Arc<Broker>,
        store: StateStore,
        source: SshKeyTargetSource,
    ) -> Result<Self> {
        Ok(Self {
            inner: TerminalSink::new(transport, selected, size, rows)?,
            broker,
            store,
            source,
        })
    }
    pub fn set_palette(&mut self, config: pulse_app::TerminalStartConfig) {
        self.inner.set_palette(config);
    }
    pub fn observe_native_admission(&self) -> Guard {
        let weak = self.inner.observe_admission();
        Arc::new(move || weak.upgrade().is_some())
    }
    pub async fn close_and_drain(&mut self) -> Result<()> {
        self.inner.close_and_drain().await
    }
}
impl InteractiveSessionSink for SessionSink {
    fn matches_target(&self, source: &SshKeyTargetSource, host: Uuid, identity: Uuid) -> bool {
        (source.supports_interactive_selection(host, identity)
            || source.supports_route_interactive_selection(host, identity))
            && self
                .inner
                .matches_target(source.password_source(), host, identity)
    }
    fn retain_native_authorization(&mut self, guard: Guard) -> Result<()> {
        self.inner.retain_native_authorization(guard)
    }
    fn authenticate_interactive<'a>(
        &'a mut self,
        username: &'a str,
        guard: Guard,
    ) -> Pin<Box<dyn Future<Output = Result<bool>> + Send + 'a>> {
        Box::pin(async move {
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
            authenticate_client(transport, &peer, &self.broker, username, guard, move || {
                let store = store.clone();
                let source = source.clone();
                let guard = original.clone();
                async move {
                    store
                        .validate_ssh_key_target_source(source, guard)
                        .await
                        .map_err(|_| VaultOwnerError::Denied)
                }
            })
            .await
        })
    }
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

pub(crate) async fn authenticate_client<F, Fut>(
    transport: &mut PinnedTransport,
    peer: &pulse_ssh::peer::VerifiedPeer,
    broker: &Arc<Broker>,
    username: &str,
    guard: Guard,
    revalidate: F,
) -> Result<bool>
where
    F: Fn() -> Fut + Send,
    Fut: Future<Output = Result<()>> + Send,
{
    if !guard() || !peer.is_current() {
        return Err(VaultOwnerError::Denied);
    }
    revalidate().await?;
    let client = transport
        .client(peer)
        .map_err(|_| VaultOwnerError::Denied)?;
    let mut response = client
        .authenticate_keyboard_interactive_start(username, None::<String>)
        .await
        .map_err(|_| VaultOwnerError::Denied)?;
    for round in 0..=8 {
        if !guard() || !peer.is_current() {
            return Err(VaultOwnerError::Denied);
        }
        revalidate().await?;
        match response {
            russh::client::KeyboardInteractiveAuthResponse::Success => return Ok(true),
            russh::client::KeyboardInteractiveAuthResponse::Failure { .. } => return Ok(false),
            russh::client::KeyboardInteractiveAuthResponse::InfoRequest {
                name,
                instructions,
                prompts,
            } if round < 8 => {
                let mut answers = broker
                    .ask(name, instructions, prompts, guard.clone())
                    .await?;
                revalidate().await?;
                if !guard() || !peer.is_current() {
                    return Err(VaultOwnerError::Denied);
                }
                response = client
                    .authenticate_keyboard_interactive_respond(std::mem::take(&mut *answers))
                    .await
                    .map_err(|_| VaultOwnerError::Denied)?;
            }
            _ => return Err(VaultOwnerError::Denied),
        }
    }
    Err(VaultOwnerError::Denied)
}

#[cfg(test)]
#[path = "ssh_interactive_broker_tests.rs"]
mod tests;
