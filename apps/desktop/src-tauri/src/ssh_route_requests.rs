//! Single original-page routed startup. Cancellation never aborts the native driver.
#[cfg(test)]
#[path = "ssh_route_requests_tests.rs"]
pub(crate) mod tests;
use super::{Confirm, Guard, Prompt};
use pulse_app::{
    registry::{PageLaunchScope, SessionId},
    vault_owner::{PageVaultOwner, SavedSessionGesture},
};
use std::sync::{
    Arc, Mutex, Weak,
    atomic::{AtomicBool, Ordering},
};
use tokio::sync::{Notify, OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use uuid::Uuid;
type Result<T> = std::result::Result<T, &'static str>;
pub(crate) struct Token {
    live: AtomicBool,
    page: Arc<PageLaunchScope>,
    expires: tokio::time::Instant,
    _permit: OwnedSemaphorePermit,
    done_consumed: Arc<Notify>,
}
impl Token {
    pub fn guard(this: &Arc<Self>) -> Guard {
        let weak = Arc::downgrade(this);
        Arc::new(move || {
            weak.upgrade().is_some_and(|token| {
                token.live.load(Ordering::Acquire) && tokio::time::Instant::now() < token.expires
            })
        })
    }
    pub fn cancel(&self) {
        self.live.store(false, Ordering::Release);
    }
}
enum Event {
    Prompt(Box<Prompt>, Uuid, oneshot::Sender<super::Decision>),
    Interactive(serde_json::Value),
    Done(Result<SessionId>),
}
struct Entry {
    id: Uuid,
    token: Arc<Token>,
    events: Option<mpsc::Receiver<Event>>,
    answer: Option<(Uuid, oneshot::Sender<super::Decision>)>,
    broker: Arc<super::super::interactive::Broker>,
}
pub(crate) struct Bank {
    capacity: Arc<Semaphore>,
    entry: Mutex<Option<Entry>>,
    closed: AtomicBool,
    runtime: tokio::runtime::Handle,
}
#[derive(serde::Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub(crate) enum Reply {
    Confirm {
        version: u8,
        challenge: String,
        prompt: Box<Prompt>,
    },
    Interactive {
        version: u8,
        prompt: serde_json::Value,
    },
    Connected {
        version: u8,
        session: String,
    },
}
pub(crate) struct CancelOnDrop {
    token: Arc<Token>,
    armed: bool,
}
impl CancelOnDrop {
    pub fn disarm(&mut self) {
        self.armed = false;
    }
}
impl Drop for CancelOnDrop {
    fn drop(&mut self) {
        if self.armed {
            self.token.cancel();
        }
    }
}
pub(crate) struct Issued {
    pub id: Uuid,
    pub captured: SavedSessionGesture,
    pub token: Arc<Token>,
    sender: mpsc::Sender<Event>,
    broker: Arc<super::super::interactive::Broker>,
}
impl Issued {
    pub fn cancellation_guard(&self) -> CancelOnDrop {
        CancelOnDrop {
            token: self.token.clone(),
            armed: true,
        }
    }
    pub fn broker(&self) -> Arc<super::super::interactive::Broker> {
        self.broker.clone()
    }
    pub fn completion_notification(&self) -> Arc<Notify> {
        self.token.done_consumed.clone()
    }
    pub fn launch<F, Fut>(self, runtime: &tokio::runtime::Handle, work: F)
    where
        F: FnOnce(SavedSessionGesture, Confirm) -> Fut + Send + 'static,
        Fut: std::future::Future<Output = Result<SessionId>> + Send + 'static,
    {
        let sender = self.sender.clone();
        let confirm: Confirm = Arc::new(move |prompt| {
            let sender = sender.clone();
            Box::pin(async move {
                let (tx, rx) = oneshot::channel();
                sender
                    .send(Event::Prompt(Box::new(prompt), Uuid::new_v4(), tx))
                    .await
                    .map_err(|_| "workspace.denied")?;
                rx.await.map_err(|_| "workspace.denied")
            })
        });
        // A strong token holds startup capacity until the real native driver and SQL tasks exit.
        runtime.spawn(async move {
            let _keep = self.token;
            let outcome = work(self.captured, confirm).await;
            let _ = self.sender.send(Event::Done(outcome)).await;
        });
    }
}
impl Bank {
    pub fn new(capacity: Arc<Semaphore>, runtime: tokio::runtime::Handle) -> Arc<Self> {
        Arc::new(Self {
            capacity,
            entry: Mutex::new(None),
            closed: AtomicBool::new(false),
            runtime,
        })
    }
    pub fn retire(&self) {
        if let Ok(mut entry) = self.entry.lock()
            && let Some(entry) = entry.take()
        {
            entry.token.cancel();
        }
    }
    pub fn close(&self) {
        self.closed.store(true, Ordering::Release);
        self.retire();
    }
    pub fn begin(
        self: &Arc<Self>,
        owner: &PageVaultOwner,
        page: Arc<PageLaunchScope>,
        host: Uuid,
        identity: Uuid,
    ) -> Result<Issued> {
        self.begin_capture(page.clone(), move |guard| {
            owner
                .begin_routed_session_gesture_authorized(page, host, identity, guard)
                .map_err(|_| "workspace.denied")
        })
    }
    pub fn begin_from_original(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        captured: SavedSessionGesture,
    ) -> Result<Issued> {
        self.begin_capture(page.clone(), move |guard| {
            captured
                .restrict_route_authorization(&page, guard)
                .map_err(|_| "workspace.denied")
        })
    }
    fn begin_capture(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        capture: impl FnOnce(Guard) -> Result<SavedSessionGesture>,
    ) -> Result<Issued> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        if self.closed.load(Ordering::Acquire) {
            return Err("workspace.denied");
        }
        if slot
            .as_ref()
            .is_some_and(|entry| !(Token::guard(&entry.token))())
        {
            slot.take();
        }
        let permit = self
            .capacity
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        let token = Arc::new(Token {
            live: AtomicBool::new(true),
            page: page.clone(),
            expires: tokio::time::Instant::now() + std::time::Duration::from_secs(300),
            _permit: permit,
            done_consumed: Arc::new(Notify::new()),
        });
        let captured = capture(Token::guard(&token))?;
        let (sender, events) = mpsc::channel(1);
        let id = Uuid::new_v4();
        let output = sender.clone();
        let broker = super::super::interactive::Broker::new(move |prompt| {
            output
                .try_send(Event::Interactive(prompt))
                .map_err(|_| pulse_app::vault_owner::VaultOwnerError::Denied)
        });
        *slot = Some(Entry {
            broker: broker.clone(),
            id,
            token: token.clone(),
            events: Some(events),
            answer: None,
        });
        let weak: Weak<Self> = Arc::downgrade(self);
        let deadline = token.expires;
        self.runtime.spawn(async move {
            tokio::time::sleep_until(deadline).await;
            if let Some(bank) = weak.upgrade()
                && let Ok(mut slot) = bank.entry.lock()
                && slot.as_ref().is_some_and(|entry| entry.id == id)
                && let Some(entry) = slot.take()
            {
                entry.token.cancel();
            }
        });
        Ok(Issued {
            id,
            captured,
            token,
            sender,
            broker,
        })
    }
    fn checked<'a>(
        &self,
        entry: &'a mut Option<Entry>,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
    ) -> Result<&'a mut Entry> {
        let entry = entry.as_mut().ok_or("workspace.denied")?;
        if self.closed.load(Ordering::Acquire)
            || entry.id != id
            || !Arc::ptr_eq(page, &entry.token.page)
            || !(Token::guard(&entry.token))()
        {
            return Err("workspace.denied");
        }
        Ok(entry)
    }
    pub fn cancel(&self, page: &Arc<PageLaunchScope>, id: Uuid) -> Result<()> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        self.checked(&mut slot, page, id)?.token.cancel();
        slot.take();
        Ok(())
    }
    pub fn answer(
        &self,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
        challenge: Uuid,
        approved: impl Into<super::Decision>,
    ) -> Result<()> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let entry = self.checked(&mut slot, page, id)?;
        if entry
            .answer
            .as_ref()
            .is_none_or(|(expected, _)| *expected != challenge)
        {
            return Err("workspace.denied");
        }
        let (_, sender) = entry.answer.take().ok_or("workspace.denied")?;
        sender.send(approved.into()).map_err(|_| "workspace.denied")
    }
    pub fn answer_interactive(
        &self,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
        round: Uuid,
        answers: zeroize::Zeroizing<Vec<String>>,
    ) -> Result<()> {
        let mut slot = self.entry.lock().map_err(|_| "workspace.denied")?;
        let entry = self.checked(&mut slot, page, id)?;
        if entry.answer.is_some() {
            return Err("workspace.denied");
        }
        entry
            .broker
            .answer(round, answers)
            .map_err(|_| "workspace.denied")
    }
    pub async fn wait(&self, page: &Arc<PageLaunchScope>, id: Uuid) -> Result<Reply> {
        let (mut events, token) = {
            let mut slot = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
            let entry = self.checked(&mut slot, page, id)?;
            if entry.answer.is_some() {
                return Err("workspace.denied");
            }
            (
                entry.events.take().ok_or("workspace.busy")?,
                entry.token.clone(),
            )
        };
        let mut drop_guard = CancelOnDrop {
            token: token.clone(),
            armed: true,
        };
        let event = tokio::select! {biased;_ = tokio::time::sleep_until(token.expires)=>return Err("workspace.denied"),event=events.recv()=>event.ok_or("workspace.ssh_failed")?};
        let mut slot = self.entry.lock().map_err(|_| "workspace.ssh_failed")?;
        let entry = self.checked(&mut slot, page, id)?;
        let reply = match event {
            Event::Prompt(prompt, challenge, sender) => {
                entry.events = Some(events);
                entry.answer = Some((challenge, sender));
                Reply::Confirm {
                    version: 1,
                    challenge: challenge.to_string(),
                    prompt,
                }
            }
            Event::Interactive(prompt) => {
                entry.events = Some(events);
                Reply::Interactive { version: 1, prompt }
            }
            Event::Done(result) => {
                slot.take();
                let session = result?;
                token.done_consumed.notify_one();
                Reply::Connected {
                    version: 1,
                    session: session.get().to_string(),
                }
            }
        };
        drop_guard.armed = false;
        Ok(reply)
    }
}
