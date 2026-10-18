//! Current-page single-use session requests. Neither a fingerprint receipt nor an auth bool can enter.
use super::kinds::{Action, Gesture, Intent, Source};
use super::starter::{StartContext, start_prepared};
use pulse_app::{
    registry::{PageLaunchScope, SessionId, SessionRegistry, WindowToken},
    vault_owner::{
        PageVaultCancellation, PageVaultOwner, PasswordSessionGesture, PrivateKeySessionGesture,
    },
};
use pulse_store::{
    StateStore, ssh_key_target_source::SshKeyTargetSource, ssh_target_source::SshTargetSource,
};
use std::{
    future::Future,
    sync::{
        Arc, Mutex, Weak,
        atomic::{AtomicBool, Ordering},
    },
};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use uuid::Uuid;
type Result<T> = std::result::Result<T, &'static str>;
type Guard = Arc<dyn Fn() -> bool + Send + Sync>;
pub(crate) struct StartTarget {
    pub root: Arc<Mutex<pulse_app::local_session_root::SessionRoot>>,
    pub size: pulse_core::TermSize,
    pub history_rows: usize,
    pub palette: pulse_app::TerminalStartConfig,
}
pub(crate) struct SessionRequests {
    registry: SessionRegistry,
    window: WindowToken,
    runtime: tokio::runtime::Handle,
    closed: AtomicBool,
    capacity: Arc<Semaphore>,
    entry: Mutex<Option<Entry>>,
}
struct Token {
    bank: Weak<SessionRequests>,
    page: Arc<PageLaunchScope>,
    expires: tokio::time::Instant,
    live: AtomicBool,
    cancel: Mutex<Option<PageVaultCancellation>>,
    cancelled: tokio_util::sync::CancellationToken,
    _permit: OwnedSemaphorePermit,
}
impl Token {
    fn current(&self) -> bool {
        self.live.load(Ordering::Acquire)
            && tokio::time::Instant::now() < self.expires
            && self.bank.upgrade().is_some_and(|bank| {
                !bank.closed.load(Ordering::Acquire)
                    && bank.registry.check_launch_page(&self.page).is_ok()
            })
    }
    fn cancel(&self) {
        self.live.store(false, Ordering::Release);
        let cancel = self
            .cancel
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone();
        if let Some(cancel) = cancel {
            cancel.cancel();
        }
    }
    fn guard(token: &Arc<Self>) -> Guard {
        let weak = Arc::downgrade(token);
        Arc::new(move || weak.upgrade().is_some_and(|t| t.current()))
    }
}
struct Ready {
    token: Arc<Token>,
    store: StateStore,
    intent: Intent,
}
enum Stage {
    Reserved(Arc<Token>, Action),
    Preparing,
    Ready(Box<Ready>),
    Starting,
}
struct Entry {
    host: Uuid,
    ticket: Uuid,
    page: Arc<PageLaunchScope>,
    token: Weak<Token>,
    interactive: Option<Arc<super::interactive::Broker>>,
    stage: Stage,
}
struct CancelRequest {
    token: Weak<Token>,
    armed: bool,
}
impl Drop for CancelRequest {
    fn drop(&mut self) {
        if self.armed
            && let Some(token) = self.token.upgrade()
        {
            token.cancel();
        }
    }
}
impl SessionRequests {
    pub fn new(
        registry: SessionRegistry,
        window: WindowToken,
        runtime: tokio::runtime::Handle,
    ) -> Arc<Self> {
        Arc::new(Self {
            registry,
            window,
            runtime,
            closed: AtomicBool::new(false),
            capacity: Arc::new(Semaphore::new(1)),
            entry: Mutex::new(None),
        })
    }
    #[cfg(test)]
    pub fn begin(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
    ) -> Result<Uuid> {
        self.begin_action(page, host, identity, owner, None)
    }
    /// Consume the original available-key gesture after a separate trusted user action.
    pub fn begin_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: PasswordSessionGesture,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::Password(gesture)),
        )
    }
    pub fn begin_key_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: PrivateKeySessionGesture,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::PrivateKey(gesture)),
        )
    }
    pub fn begin_agent_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: pulse_app::vault_owner::AgentSessionGesture,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(page, host, identity, owner, Some(Gesture::Agent(gesture)))
    }
    pub fn begin_interactive_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: pulse_app::vault_owner::InteractiveSessionGesture,
        broker: Arc<super::interactive::Broker>,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::Interactive(gesture, broker)),
        )
    }
    pub fn begin_password_interactive_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: pulse_app::vault_owner::PasswordInteractiveSessionGesture,
        broker: Arc<super::interactive::Broker>,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::PasswordInteractive(gesture, broker)),
        )
    }
    pub fn begin_key_interactive_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: pulse_app::vault_owner::PrivateKeyInteractiveSessionGesture,
        broker: Arc<super::interactive::Broker>,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::KeyInteractive(gesture, broker)),
        )
    }
    pub fn begin_agent_interactive_gesture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: pulse_app::vault_owner::AgentInteractiveSessionGesture,
        broker: Arc<super::interactive::Broker>,
    ) -> Result<Uuid> {
        if !gesture.matches_selection(host, identity) {
            return Err("workspace.denied");
        }
        self.begin_action(
            page,
            host,
            identity,
            owner,
            Some(Gesture::AgentInteractive(gesture, broker)),
        )
    }
    fn begin_action(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
        owner: &PageVaultOwner,
        gesture: Option<Gesture>,
    ) -> Result<Uuid> {
        let interactive = gesture.as_ref().and_then(|g| match g {
            Gesture::Interactive(_, broker)
            | Gesture::PasswordInteractive(_, broker)
            | Gesture::KeyInteractive(_, broker)
            | Gesture::AgentInteractive(_, broker) => Some(broker.clone()),
            _ => None,
        });
        let lifetime = if interactive.is_some() { 300 } else { 5 };
        if self.closed.load(Ordering::Acquire)
            || page.owner() != self.window
            || self.registry.check_launch_page(&page).is_err()
            || [host, identity]
                .iter()
                .any(|id| id.get_version_num() != 7 || id.get_variant() != uuid::Variant::RFC4122)
        {
            return Err("workspace.denied");
        }
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let token = Arc::new(Token {
            bank: Arc::downgrade(self),
            page: page.clone(),
            expires: tokio::time::Instant::now() + std::time::Duration::from_secs(lifetime),
            live: AtomicBool::new(true),
            cancel: Mutex::new(None),
            cancelled: tokio_util::sync::CancellationToken::new(),
            _permit: permit,
        });
        let action = match gesture {
            Some(gesture) => gesture.begin_session_authorized(owner, &page, Token::guard(&token)),
            None => owner
                .begin_password_session_action_authorized(
                    page.clone(),
                    host,
                    identity,
                    Token::guard(&token),
                )
                .map(Action::Password),
        }
        .map_err(|_| "workspace.ssh_denied")?;
        *token.cancel.lock().map_err(|_| "workspace.ssh_failed")? = Some(action.cancellation());
        let ticket = Uuid::new_v4();
        let deadline = token.expires;
        {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            if entry.is_some() || !token.current() {
                return Err("workspace.denied");
            };
            *entry = Some(Entry {
                host,
                ticket,
                page,
                token: Arc::downgrade(&token),
                interactive,
                stage: Stage::Reserved(token, action),
            });
        }
        let bank = Arc::downgrade(self);
        self.runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(bank) = bank.upgrade() {
                bank.remove(ticket);
            }
        });
        Ok(ticket)
    }
    fn checked<'a>(
        &self,
        entry: &'a mut Option<Entry>,
        page: &Arc<PageLaunchScope>,
        ticket: Uuid,
    ) -> Result<&'a mut Entry> {
        let e = entry.as_mut().ok_or("workspace.denied")?;
        if e.ticket != ticket
            || !Arc::ptr_eq(&e.page, page)
            || !e.token.upgrade().is_some_and(|token| token.current())
        {
            return Err("workspace.denied");
        };
        Ok(e)
    }
    pub fn selected_host(&self, page: &Arc<PageLaunchScope>, ticket: Uuid) -> Result<Uuid> {
        let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        Ok(self.checked(&mut entry, page, ticket)?.host)
    }
    pub fn authorization(&self, page: &Arc<PageLaunchScope>, ticket: Uuid) -> Result<Guard> {
        let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let e = self.checked(&mut entry, page, ticket)?;
        if !matches!(e.stage, Stage::Reserved(..)) {
            return Err("workspace.denied");
        };
        Ok(Token::guard(&e.token.upgrade().ok_or("workspace.denied")?))
    }
    fn validate_role(
        &self,
        page: &Arc<PageLaunchScope>,
        ticket: Uuid,
        private_key: bool,
    ) -> Result<()> {
        let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let e = self.checked(&mut entry, page, ticket)?;
        match (&e.stage, private_key) {
            (Stage::Reserved(_, Action::Password(_) | Action::PasswordInteractive(..)), false)
            | (Stage::Reserved(_, Action::PrivateKey(_) | Action::KeyInteractive(..)), true) => {
                Ok(())
            }
            _ => Err("workspace.denied"),
        }
    }
    pub async fn prepare<F>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        load: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<(StateStore, SshTargetSource)>> + Send + 'static,
    {
        self.validate_role(&page, ticket, false)?;
        self.prepare_typed(page, ticket, async move {
            load.await
                .map(|(store, source)| (store, Source::Password(source)))
        })
        .await
    }
    pub async fn prepare_key<F>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        load: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<(StateStore, SshKeyTargetSource)>> + Send + 'static,
    {
        self.validate_role(&page, ticket, true)?;
        self.prepare_typed(page, ticket, async move {
            load.await
                .map(|(store, source)| (store, Source::PrivateKey(source)))
        })
        .await
    }
    pub async fn prepare_agent<F>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        load: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<(StateStore, SshKeyTargetSource)>> + Send + 'static,
    {
        {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let e = self.checked(&mut entry, &page, ticket)?;
            if !matches!(
                &e.stage,
                Stage::Reserved(_, Action::Agent(_))
                    | Stage::Reserved(_, Action::AgentInteractive(_, _))
            ) {
                return Err("workspace.denied");
            }
        }
        self.prepare_typed(page, ticket, async move {
            load.await
                .map(|(store, source)| (store, Source::Agent(source)))
        })
        .await
    }
    pub async fn prepare_interactive<F>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        load: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<(StateStore, SshKeyTargetSource)>> + Send + 'static,
    {
        {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let e = self.checked(&mut entry, &page, ticket)?;
            if !matches!(&e.stage, Stage::Reserved(_, Action::Interactive(..))) {
                return Err("workspace.denied");
            }
        }
        self.prepare_typed(page, ticket, async move {
            load.await
                .map(|(store, source)| (store, Source::Interactive(source)))
        })
        .await
    }
    async fn prepare_typed<F>(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        load: F,
    ) -> Result<()>
    where
        F: Future<Output = Result<(StateStore, Source)>> + Send + 'static,
    {
        let (token, action) = {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let e = self.checked(&mut entry, &page, ticket)?;
            if !matches!(e.stage, Stage::Reserved(..)) {
                return Err("workspace.denied");
            };
            let Stage::Reserved(token, action) = std::mem::replace(&mut e.stage, Stage::Preparing)
            else {
                return Err("workspace.denied");
            };
            (token, action)
        };
        let mut cancellation = CancelRequest {
            token: Arc::downgrade(&token),
            armed: true,
        };
        let bank = self.clone();
        let work = self.runtime.spawn(async move {
            let outcome = async {
                if !token.current() {
                    return Err("workspace.denied");
                };
                let (store, source) = load.await?;
                let intent = action
                    .prepare(&store, source)
                    .await
                    .map_err(|_| "workspace.ssh_denied")?;
                let mut entry = bank.entry.lock().map_err(|_| "workspace.ssh_failed")?;
                let e = bank.checked(&mut entry, &page, ticket)?;
                if !matches!(e.stage, Stage::Preparing) {
                    return Err("workspace.denied");
                };
                e.stage = Stage::Ready(Box::new(Ready {
                    token: token.clone(),
                    store,
                    intent,
                }));
                Ok(())
            }
            .await;
            if outcome.is_err() {
                bank.remove(ticket)
            };
            outcome
        });
        let result = work.await.map_err(|_| "workspace.ssh_failed")?;
        cancellation.armed = false;
        result
    }
    pub async fn start(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        ticket: Uuid,
        target: StartTarget,
    ) -> Result<SessionId> {
        let ready = {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let e = self.checked(&mut entry, &page, ticket)?;
            if !matches!(e.stage, Stage::Ready(_)) {
                return Err("workspace.denied");
            };
            let Stage::Ready(ready) = std::mem::replace(&mut e.stage, Stage::Starting) else {
                return Err("workspace.denied");
            };
            ready
        };
        let Ready {
            token,
            store,
            intent,
        } = *ready;
        let context = StartContext {
            store,
            registry: self.registry.clone(),
            page,
            root: target.root,
            size: target.size,
            history_rows: target.history_rows,
            palette: target.palette,
        };
        let mut cancellation = CancelRequest {
            token: Arc::downgrade(&token),
            armed: true,
        };
        let bank = self.clone();
        let work = self.runtime.spawn(async move {
            let _actual_lifetime = &token;
            let outcome = match start_prepared(context, intent) {
                Ok(task) => task.wait().await,
                Err(error) => Err(error),
            };
            bank.remove(ticket);
            outcome
        });
        let result = work.await.map_err(|_| "workspace.ssh_failed")?;
        cancellation.armed = false;
        result
    }
    /// Explicit request cancellation only; observing it retains no native permit.
    /// Normal completion removes the entry without signalling cancellation.
    pub fn cancellation(
        &self,
        page: &Arc<PageLaunchScope>,
        ticket: Uuid,
    ) -> Result<tokio_util::sync::CancellationToken> {
        let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let token = self
            .checked(&mut entry, page, ticket)?
            .token
            .upgrade()
            .ok_or("workspace.denied")?;
        Ok(token.cancelled.clone())
    }
    pub fn cancel(&self, page: &Arc<PageLaunchScope>, ticket: Uuid) -> Result<()> {
        let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let token = self
            .checked(&mut entry, page, ticket)?
            .token
            .upgrade()
            .ok_or("workspace.denied")?;
        drop(entry);
        token.cancelled.cancel();
        self.remove(ticket);
        Ok(())
    }
    pub fn answer_interactive(
        &self,
        page: &Arc<PageLaunchScope>,
        ticket: Uuid,
        round: Uuid,
        answers: zeroize::Zeroizing<Vec<String>>,
    ) -> Result<()> {
        let broker = {
            let mut entry = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let e = self.checked(&mut entry, page, ticket)?;
            if !matches!(e.stage, Stage::Starting) {
                return Err("workspace.denied");
            }
            e.interactive.clone().ok_or("workspace.denied")?
        };
        broker
            .answer(round, answers)
            .map_err(|_| "workspace.denied")
    }
    fn remove(&self, ticket: Uuid) {
        let old = {
            let mut entry = self.entry.lock().unwrap_or_else(|p| p.into_inner());
            if entry.as_ref().is_some_and(|e| e.ticket == ticket) {
                entry.take()
            } else {
                None
            }
        };
        if let Some(entry) = old
            && let Some(token) = entry.token.upgrade()
        {
            token.cancel();
        }
    }
    pub fn retire(&self) {
        let old = self.entry.lock().unwrap_or_else(|p| p.into_inner()).take();
        if let Some(entry) = old
            && let Some(token) = entry.token.upgrade()
        {
            token.cancelled.cancel();
            token.cancel();
        }
    }
    pub(crate) fn startup_capacity(&self) -> Arc<Semaphore> {
        self.capacity.clone()
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.retire();
    }
    pub async fn shutdown(&self) -> Result<()> {
        self.close();
        let _actual_exit = self
            .capacity
            .clone()
            .acquire_owned()
            .await
            .map_err(|_| "workspace.ssh_failed")?;
        Ok(())
    }
}
#[cfg(test)]
mod runtime_tests {
    use super::*;
    #[test]
    fn synchronous_begin_outside_tokio_context_uses_explicit_runtime()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()?;
        assert!(tokio::runtime::Handle::try_current().is_err());
        let registry = SessionRegistry::new();
        let window = registry.open_window()?;
        let documents = pulse_app::product_document::ProductDocuments::new(registry.clone());
        documents.install(window)?;
        documents.started(window)?;
        let nonce = documents.finished(window)?;
        documents.confirm(window, nonce)?;
        let page = documents.current(window, nonce)?;
        let vault = PageVaultOwner::new(registry.clone());
        let bank = SessionRequests::new(registry.clone(), window, runtime.handle().clone());
        let ticket = bank.begin(page.clone(), Uuid::now_v7(), Uuid::now_v7(), &vault)?;
        assert!(bank.authorization(&page, ticket).is_ok());
        let cancelled = bank.cancellation(&page, ticket)?;
        let native_token = bank
            .entry
            .lock()
            .map_err(|_| "poisoned bank")?
            .as_ref()
            .and_then(|entry| entry.token.upgrade())
            .ok_or("missing native token")?;
        bank.cancel(&page, ticket)?;
        assert!(cancelled.is_cancelled());
        // A cancelled CLI waiter must not release a still-owned native worker slot.
        assert_eq!(bank.startup_capacity().available_permits(), 0);
        assert!(
            bank.begin(page.clone(), Uuid::now_v7(), Uuid::now_v7(), &vault)
                .is_err()
        );
        drop(native_token);
        let completed = bank.begin(page.clone(), Uuid::now_v7(), Uuid::now_v7(), &vault)?;
        let normal = bank.cancellation(&page, completed)?;
        bank.remove(completed);
        assert!(
            !normal.is_cancelled(),
            "normal completion is not explicit cancellation"
        );
        runtime.block_on(bank.shutdown())?;
        vault.close()?;
        documents.remove(window)?;
        registry.close_window(window)?;
        registry.shutdown()?;
        Ok(())
    }
}
