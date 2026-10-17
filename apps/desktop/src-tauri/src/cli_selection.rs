//! Keyless original-page selection receipts. No session, input or launch authority.
//! The adapter must independently check root session ownership before requesting
//! selection and again before publication. Sending an event is not completion.
use pulse_app::{
    product_document::ProductDocuments,
    registry::{PageLaunchScope, WindowToken},
};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::sync::oneshot;
use tokio_util::sync::CancellationToken;
use uuid::Uuid;

/// Content-free selection rejection; no request or credential data.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
#[error("workspace.cli_selection_unconfirmed")]
pub struct Error;
struct Pending {
    id: Uuid,
    page: Arc<PageLaunchScope>,
    session: u64,
    until: Instant,
    stop: CancellationToken,
    admitted: bool,
    answer: oneshot::Sender<Result<(), Error>>,
}
#[derive(Default)]
struct State {
    pending: Option<Pending>,
    closed: bool,
}
/// One bounded selection attempt per root. This holds no native I/O credit.
pub struct Selections {
    documents: Arc<ProductDocuments>,
    owner: WindowToken,
    state: Mutex<State>,
}
/// Single-use original-page receipt waiter; dropping it cancels only its own slot.
pub struct Reservation {
    bank: Arc<Selections>,
    /// Opaque correlation id, not authority to select any session.
    pub id: Uuid,
    /// Completion is supplied only by the validated original page, not event delivery.
    pub answer: oneshot::Receiver<Result<(), Error>>,
}
impl Drop for Reservation {
    fn drop(&mut self) {
        if let Ok(mut state) = self.bank.state.lock()
            && state
                .pending
                .as_ref()
                .is_some_and(|pending| pending.id == self.id)
        {
            state.pending.take();
        }
    }
}
impl Selections {
    /// Bind permanently to the root's exact document controller and window identity.
    pub fn new(documents: Arc<ProductDocuments>, owner: WindowToken) -> Self {
        Self {
            documents,
            owner,
            state: Mutex::new(State::default()),
        }
    }
    fn current(&self, page: &Arc<PageLaunchScope>) -> Result<(), Error> {
        let current = self
            .documents
            .current_native(self.owner)
            .map_err(|_| Error)?;
        if !Arc::ptr_eq(&current, page) {
            return Err(Error);
        }
        Ok(())
    }
    /// Reserve one already-authorized root session id; does not itself prove ownership.
    pub fn begin(
        self: &Arc<Self>,
        page: Arc<PageLaunchScope>,
        session: u64,
        stop: CancellationToken,
    ) -> Result<Reservation, Error> {
        self.current(&page)?;
        if session == 0 || stop.is_cancelled() {
            return Err(Error);
        }
        let mut state = self.state.lock().map_err(|_| Error)?;
        if state.closed || state.pending.is_some() {
            return Err(Error);
        }
        let id = Uuid::new_v4();
        let (answer, receive) = oneshot::channel();
        state.pending = Some(Pending {
            id,
            page,
            session,
            until: Instant::now() + Duration::from_secs(5),
            stop,
            admitted: false,
            answer,
        });
        Ok(Reservation {
            bank: self.clone(),
            id,
            answer: receive,
        })
    }
    /// Revoke pending selection on navigation without sealing the next page's bank.
    pub fn retire(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.pending.take();
        }
    }
    /// Root closure permanently prevents future reservation and revokes current waiters.
    pub fn close(&self) {
        if let Ok(mut state) = self.state.lock() {
            state.closed = true;
            state.pending.take();
        }
    }
    /// Consume the original page's one-use UI admission before any selection effect.
    /// Caller validates actual source and root ownership; this never enables terminal input.
    pub fn admit(&self, page: &Arc<PageLaunchScope>, id: Uuid, session: u64) -> Result<(), Error> {
        self.current(page)?;
        let mut state = self.state.lock().map_err(|_| Error)?;
        let pending = state.pending.as_mut().ok_or(Error)?;
        if pending.id != id
            || pending.session != session
            || !Arc::ptr_eq(&pending.page, page)
            || pending.admitted
            || pending.stop.is_cancelled()
            || Instant::now() >= pending.until
            || pending.answer.is_closed()
        {
            return Err(Error);
        }
        pending.admitted = true;
        Ok(())
    }
    /// Accept exact original page/ticket/target completion once. Caller validates source.
    /// False, late, cancelled or lost-waiter results never become selection success.
    pub fn complete(
        &self,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
        session: u64,
        selected: bool,
    ) -> Result<(), Error> {
        self.complete_at(page, id, session, selected, Instant::now())
    }
    fn complete_at(
        &self,
        page: &Arc<PageLaunchScope>,
        id: Uuid,
        session: u64,
        selected: bool,
        now: Instant,
    ) -> Result<(), Error> {
        self.current(page)?;
        let mut state = self.state.lock().map_err(|_| Error)?;
        let pending = state.pending.as_ref().ok_or(Error)?;
        if pending.id != id || pending.session != session || !Arc::ptr_eq(&pending.page, page) {
            return Err(Error);
        }
        if pending.stop.is_cancelled() || now >= pending.until || pending.answer.is_closed() {
            state.pending.take();
            return Err(Error);
        }
        if selected && !pending.admitted {
            return Err(Error);
        }
        let pending = state.pending.take().ok_or(Error)?;
        pending
            .answer
            .send(if selected { Ok(()) } else { Err(Error) })
            .map_err(|_| Error)
    }
}
#[cfg(test)]
#[path = "cli_selection_tests.rs"]
mod tests;
