//! Supervised, window-neutral diagnostic wire binding. Callers own permissions,
//! globally fresh generations, binary delivery, and renderer invalidation.
use pulse_ipc::{
    attach::{AttachError, AttachmentSender, Boundary, ENVELOPE_LEN, MAX_FRAGMENT},
    flow::FlowError,
};
use pulse_term::session::{
    Lifecycle, SessionHandle, SessionRestoreError, SessionState, SessionStream, StreamEnd,
    StreamRecord,
};
#[cfg(test)]
#[path = "attachment_tests.rs"]
mod tests;
use std::{collections::VecDeque, time::Duration};
use tokio::{
    sync::{mpsc, watch},
    task::JoinHandle,
    time::Instant,
};

/// Hard caps apply also when callers request larger windows.
#[derive(Clone, Copy, Debug)]
pub struct AttachmentConfig {
    /// Wire fragment body bytes, 1..=64 KiB.
    pub fragment: usize,
    /// Maximum outstanding wire frames, 1..=8.
    pub frames: usize,
    /// Envelope/body credit bytes, up to 1 MiB; outer headers are separate.
    pub bytes: usize,
    /// Deadline of the oldest outstanding frame; 1 ms..=30 seconds.
    pub ack_timeout: Duration,
}
impl Default for AttachmentConfig {
    fn default() -> Self {
        Self {
            fragment: MAX_FRAGMENT,
            frames: 8,
            bytes: 512 * 1024,
            ack_timeout: Duration::from_secs(5),
        }
    }
}
impl AttachmentConfig {
    fn validate(self) -> Result<(), AttachmentError> {
        if !(1..=MAX_FRAGMENT).contains(&self.fragment)
            || !(1..=8).contains(&self.frames)
            || self.bytes < self.fragment.max(28) + ENVELOPE_LEN
            || self.bytes > 1024 * 1024
            || self.ack_timeout < Duration::from_millis(1)
            || self.ack_timeout > Duration::from_secs(30)
        {
            return Err(AttachmentError::Limits);
        }
        Ok(())
    }
}
/// Driver failures invalidate the whole renderer generation, never the session.
#[derive(Debug, thiserror::Error)]
pub enum AttachmentError {
    /// Unbounded or nonviable window/deadline.
    #[error("invalid attachment limits")]
    Limits,
    /// Actor refused the diagnostic capture. The previous lease is preserved.
    #[error(transparent)]
    Capture(#[from] SessionRestoreError),
    /// Wire/credit violation, including wrong generation and duplicate ACK.
    #[error(transparent)]
    Protocol(#[from] AttachError),
    /// No valid cumulative ACK before the oldest frame's fixed deadline.
    #[error("attachment acknowledgment deadline")]
    AckTimeout,
    /// Viewer cancelled or disappeared, without closing the terminal.
    #[error("attachment detached")]
    Detached,
    /// Lease invalidated; queued frames must be discarded with the old surface.
    #[error("attachment invalidated: {0:?}")]
    Invalidated(StreamEnd),
    /// Local supervisor task failed.
    #[error("attachment supervisor failed")]
    Supervisor,
    /// Bounded ACK admission failed. Not a remote ACK acceptance receipt.
    #[error("attachment acknowledgment queue full")]
    Busy,
}
/// Completion proves all emitted frames including End received valid ACKs.
/// Callers must submit ACK only after the consumer's render callback.
#[derive(Debug)]
pub struct AttachmentReceipt {
    /// Final actor state after genuine EOF.
    pub boundary: SessionState,
    /// Includes bootstrap, data, checkpoints and End.
    pub frames: u64,
    /// Observed maximum envelope/body bytes in flight, not RSS.
    pub peak_credit_bytes: usize,
}
/// Cloneable bound ACK/cancellation endpoint, not a permission token.
#[derive(Clone)]
pub struct AttachmentHandle {
    session: SessionHandle,
    lease_id: u64,
    ack: mpsc::Sender<(u64, u32)>,
    cancel: watch::Sender<bool>,
}
impl AttachmentHandle {
    /// Admission only: the driver validates generation and outstanding sequence.
    pub fn acknowledge(&self, generation: u64, sequence: u32) -> Result<(), AttachmentError> {
        self.ack
            .try_send((generation, sequence))
            .map_err(|error| match error {
                mpsc::error::TrySendError::Full(_) => AttachmentError::Busy,
                mpsc::error::TrySendError::Closed(_) => AttachmentError::Detached,
            })
    }
    /// Atomic same-boundary comparison and ordered exact repair admission.
    pub async fn request_repair(
        &self,
        boundary: SessionState,
        digest: String,
    ) -> Result<pulse_term::session::RepairAdmission, AttachmentError> {
        if *self.cancel.borrow() {
            return Err(AttachmentError::Detached);
        }
        self.session
            .diagnostic_repair(pulse_term::session::SessionRepairRequest {
                lease_id: self.lease_id,
                boundary,
                digest,
                cancelled: self.cancel.subscribe(),
            })
            .await
            .map_err(AttachmentError::Capture)
    }
    /// Native ACK commits dimensions, invalidating this stream before reattachment.
    pub async fn resize(&self, size: pulse_core::TermSize) -> Result<(), AttachmentError> {
        if *self.cancel.borrow() {
            return Err(AttachmentError::Detached);
        }
        self.session
            .resize_for_lease(self.lease_id, self.cancel.subscribe(), size)
            .await
            .map_err(|e| AttachmentError::Capture(e.into()))
    }

    /// Bound user input: actor checks lease and this cancellation before admission.
    pub async fn input(&self, bytes: Vec<u8>) -> Result<(), AttachmentError> {
        if *self.cancel.borrow() {
            return Err(AttachmentError::Detached);
        }
        self.session
            .input_for_lease(self.lease_id, self.cancel.subscribe(), bytes)
            .await
            .map_err(|e| AttachmentError::Capture(e.into()))
    }

    /// Actor admission only; Marker delivery and rendering still require wire ACK.
    pub async fn request_marker(&self) -> Result<(), AttachmentError> {
        if *self.cancel.borrow() {
            return Err(AttachmentError::Detached);
        }
        self.session
            .diagnostic_marker(self.lease_id)
            .await
            .map_err(|e| AttachmentError::Capture(e.into()))
    }
    /// Out-of-band revocation; does not close the registry/session handle.
    pub fn detach(&self) {
        self.cancel.send_replace(true);
    }
}
/// Drop requests revocation; join observes the complete result without aborting
/// native/session workers. Applications must observe the receipt to revoke surfaces.
pub struct AttachmentTask {
    cancel: watch::Sender<bool>,
    task: Option<JoinHandle<Result<AttachmentReceipt, AttachmentError>>>,
}
impl Drop for AttachmentTask {
    fn drop(&mut self) {
        self.cancel.send_replace(true);
    }
}
impl AttachmentTask {
    /// Completion or failure. Cancelling this future revokes this binding.
    pub async fn join(mut self) -> Result<AttachmentReceipt, AttachmentError> {
        self.task
            .take()
            .ok_or(AttachmentError::Supervisor)?
            .await
            .map_err(|_| AttachmentError::Supervisor)?
    }
}
/// Atomically capture bounded recent history/screen and subscribe its post-boundary
/// output. Config/generation are validated BEFORE replacing an existing lease.
/// This is diagnostic UTF-8 binding, not the window/permission registry API.
pub async fn attach(
    session: &SessionHandle,
    generation: u64,
    config: AttachmentConfig,
) -> Result<(AttachmentHandle, mpsc::Receiver<Vec<u8>>, AttachmentTask), AttachmentError> {
    config.validate()?;
    if generation == 0 {
        return Err(AttachError::Generation.into());
    }
    let mut stream = session
        .diagnostic_subscribe(2000, 4 * 1024 * 1024, 1024 * 1024)
        .await?;
    let lease_id = stream.lease_id;
    let bootstrap = &stream.bootstrap;
    let sender = AttachmentSender::new(
        generation,
        boundary(&bootstrap.boundary),
        std::mem::take(&mut stream.bootstrap.history.bytes),
        std::mem::take(&mut stream.bootstrap.restore),
        config.fragment,
        config.frames,
        config.bytes,
    )?;
    let (wire, receiver) = mpsc::channel(config.frames);
    let (ack, acks) = mpsc::channel(config.frames);
    let (cancel, cancellation) = watch::channel(false);
    let task = tokio::spawn(
        Driver {
            stream,
            sender,
            wire,
            acks,
            cancellation,
            config,
            deadlines: VecDeque::new(),
            total: 0,
            peak: 0,
            repair_hold: None,
            repair_acks: 0,
            repair_sending: false,
        }
        .run(),
    );
    Ok((
        AttachmentHandle {
            session: session.clone(),
            lease_id,
            ack,
            cancel: cancel.clone(),
        },
        receiver,
        AttachmentTask {
            cancel,
            task: Some(task),
        },
    ))
}
fn boundary(state: &SessionState) -> Boundary {
    Boundary {
        columns: state.size.columns(),
        rows: state.size.rows(),
        revision: state.revision,
        parsed_bytes: state.parsed_bytes,
    }
}
struct Driver {
    stream: SessionStream,
    sender: AttachmentSender,
    wire: mpsc::Sender<Vec<u8>>,
    acks: mpsc::Receiver<(u64, u32)>,
    cancellation: watch::Receiver<bool>,
    config: AttachmentConfig,
    deadlines: VecDeque<Instant>,
    total: u64,
    peak: usize,
    repair_hold: Option<tokio::sync::OwnedSemaphorePermit>,
    repair_acks: usize,
    repair_sending: bool,
}
impl Driver {
    fn acknowledge(&mut self, ack: (u64, u32)) -> Result<(), AttachmentError> {
        let release = self.sender.acknowledge(ack.0, ack.1)?;
        if release.frames > self.deadlines.len() {
            return Err(AttachError::Flow(FlowError::InvalidAck).into());
        }
        for _ in 0..release.frames {
            self.deadlines.pop_front();
        }
        if self.repair_acks > 0 {
            self.repair_acks = self.repair_acks.saturating_sub(release.frames);
            if self.repair_acks == 0 {
                self.repair_hold = None;
            }
        }
        Ok(())
    }
    async fn run(mut self) -> Result<AttachmentReceipt, AttachmentError> {
        let mut bootstrap = true;
        let mut pending: Option<Vec<u8>> = None;
        let mut pending_deadline = None;
        let mut record: Option<StreamRecord> = None;
        let mut at = 0;
        let mut live_cursor = self.stream.bootstrap.boundary.parsed_bytes;
        let mut drained = false;
        let mut ended = false;
        let mut status_open = true;
        loop {
            if *self.cancellation.borrow() {
                return Err(AttachmentError::Detached);
            }
            let status = self.stream.status.borrow().clone();
            match status.end {
                StreamEnd::Open | StreamEnd::Session(Lifecycle::Ended) => {}
                other => return Err(AttachmentError::Invalidated(other)),
            }
            if self
                .deadlines
                .front()
                .is_some_and(|deadline| *deadline <= Instant::now())
            {
                return Err(AttachmentError::AckTimeout);
            }
            if ended && pending.is_none() && self.deadlines.is_empty() {
                return Ok(AttachmentReceipt {
                    boundary: status.boundary,
                    frames: self.total,
                    peak_credit_bytes: self.peak,
                });
            }
            if !bootstrap
                && !self.repair_sending
                && self.repair_acks == 0
                && matches!(record, Some(StreamRecord::Repair { .. }))
                && pending.is_none()
                && let Some(StreamRecord::Repair {
                    boundary: state,
                    bytes,
                    hold,
                }) = record.take()
            {
                if state.parsed_bytes != live_cursor {
                    return Err(AttachError::Invalid.into());
                }
                self.sender.start_repair(boundary(&state), bytes)?;
                self.repair_hold = hold;
                self.repair_sending = true;
            }
            if pending.is_none() && !ended {
                let next = if bootstrap {
                    self.sender.next_frame().inspect(|frame| {
                        if frame.is_none() {
                            bootstrap = false;
                        }
                    })
                } else if self.repair_sending {
                    match self.sender.next_repair() {
                        Ok(None) => {
                            self.repair_sending = false;
                            self.repair_acks = self.deadlines.len();
                            if self.repair_acks == 0 {
                                self.repair_hold = None;
                            }
                            continue;
                        }
                        other => other,
                    }
                } else if let Some(ref value) = record {
                    match value {
                        StreamRecord::Data {
                            start,
                            bytes,
                            boundary: state,
                        } => {
                            if *start != live_cursor
                                || start.checked_add(bytes.len() as u64) != Some(state.parsed_bytes)
                                || state.size != self.stream.bootstrap.boundary.size
                            {
                                return Err(AttachError::Invalid.into());
                            }
                            let next_cursor = state.parsed_bytes;
                            let len = bytes.len();
                            let end = (at + self.config.fragment).min(len);
                            self.sender.data(&bytes[at..end]).map(|frame| {
                                at = end;
                                if at == len {
                                    live_cursor = next_cursor;
                                    record = None;
                                    at = 0;
                                }
                                Some(frame)
                            })
                        }
                        StreamRecord::Repair { .. } => return Err(AttachError::Phase.into()),
                        StreamRecord::Marker(state) => {
                            self.sender.marker(boundary(state)).map(|frame| {
                                record = None;
                                Some(frame)
                            })
                        }
                        StreamRecord::Fence(state) => {
                            self.sender.checkpoint(boundary(state)).map(|frame| {
                                record = None;
                                Some(frame)
                            })
                        }
                    }
                } else if drained {
                    if status.end != StreamEnd::Session(Lifecycle::Ended) {
                        return Err(AttachmentError::Invalidated(status.end));
                    }
                    self.sender.finish(boundary(&status.boundary)).map(|frame| {
                        ended = true;
                        Some(frame)
                    })
                } else {
                    Ok(None)
                };
                match next {
                    Ok(Some(frame)) => {
                        pending_deadline = Some(Instant::now() + self.config.ack_timeout);
                        self.total += 1;
                        self.peak = self.peak.max(self.sender.outstanding_bytes());
                        pending = Some(frame);
                    }
                    Ok(None) => {}
                    Err(AttachError::Flow(FlowError::NoCredit)) => {}
                    Err(error) => return Err(error.into()),
                }
                // Commit->live is a local transition, not an event to wait for.
                if !bootstrap
                    && pending.is_none()
                    && record.is_none()
                    && !self.repair_sending
                    && self.repair_acks == 0
                    && !drained
                    && let Ok(value) = self.stream.output.try_recv()
                {
                    record = Some(value);
                    continue;
                }
            }
            let deadline = self.deadlines.front().copied().or(pending_deadline);
            let wire = self.wire.clone();
            tokio::select! {
                biased;
                _=self.cancellation.changed()=>return Err(AttachmentError::Detached),
                changed=self.stream.status.changed(), if status_open=>{if changed.is_err(){status_open=false;}},
                ack=self.acks.recv()=>self.acknowledge(ack.ok_or(AttachmentError::Detached)?)?,
                _=async {match deadline{Some(at)=>tokio::time::sleep_until(at).await,None=>std::future::pending().await}}=>return Err(AttachmentError::AckTimeout),
                _=self.wire.closed()=>return Err(AttachmentError::Detached),
                permit=wire.reserve(), if pending.is_some()=>{permit.map_err(|_|AttachmentError::Detached)?.send(pending.take().ok_or(AttachmentError::Supervisor)?);self.deadlines.push_back(pending_deadline.take().ok_or(AttachmentError::Supervisor)?);},
                value=self.stream.output.recv(), if !bootstrap && record.is_none() && !self.repair_sending && self.repair_acks == 0 && !drained && pending.is_none()=>{match value{Some(value)=>record=Some(value),None=>drained=true,}},
            }
        }
    }
}
