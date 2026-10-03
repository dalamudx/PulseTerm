//! Finite viewer lifecycle metadata. No terminal lock or frontend owner token.
use std::sync::{
    Mutex,
    atomic::{AtomicBool, AtomicU64, Ordering},
};
use tokio::sync::watch;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy)]
struct Pause {
    generation: u64,
    resumed: bool,
    native_started: bool,
    native_inflight: bool,
}
pub(crate) struct VisibilityGate {
    paused: Mutex<Option<Pause>>,
    wake: watch::Sender<u64>,
    closed: AtomicBool,
    native_checked: AtomicBool,
    external: AtomicU64,
}
impl Default for VisibilityGate {
    fn default() -> Self {
        let (wake, _) = watch::channel(0);
        Self {
            paused: Mutex::new(None),
            wake,
            closed: AtomicBool::new(false),
            native_checked: AtomicBool::new(false),
            external: AtomicU64::new(0),
        }
    }
}
impl VisibilityGate {
    pub(crate) fn pause(&self, generation: u64, revoke: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        self.ensure_open()?;
        if paused.is_some() {
            return Err("view.already_paused".into());
        }
        let external = self.external.load(Ordering::Acquire);
        if external != 0 && external != generation {
            return Err("view.stale_external".into());
        }
        revoke()?;
        *paused = Some(Pause {
            generation,
            resumed: false,
            native_started: external != 0,
            native_inflight: external != 0,
        });
        self.wake.send_replace(generation);
        Ok(())
    }
    pub(crate) fn hidden(&self, generation: u64) -> Result<()> {
        self.ensure_open()?;
        if self
            .paused
            .lock()
            .map_err(|_| "view.poisoned")?
            .is_some_and(|p| p.generation == generation && !p.resumed)
        {
            Ok(())
        } else {
            Err("view.stale_visibility".into())
        }
    }
    pub(crate) fn resume(&self, generation: u64) -> Result<()> {
        let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        self.ensure_open()?;
        let p = paused
            .as_mut()
            .filter(|p| p.generation == generation && !p.resumed && !p.native_inflight)
            .ok_or("view.stale_visibility")?;
        p.resumed = true;
        self.wake.send_replace(generation);
        Ok(())
    }
    fn ensure_open(&self) -> Result<()> {
        if self.closed.load(Ordering::Acquire) {
            Err("view.closed".into())
        } else {
            Ok(())
        }
    }
    pub(crate) fn close(&self) {
        // Destruction must still invalidate and wake observers after a panic.
        let mut paused = self
            .paused
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.closed.store(true, Ordering::Release);
        *paused = None;
        self.wake.send_replace(0);
    }
    /// Reserve a native window action, NOT a terminal pause. Only the frontend's
    /// event-driven pause may detach and publish this generation's pause receipt.
    pub(crate) fn reserve_external(&self, generation: u64) -> Result<()> {
        let paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        self.ensure_open()?;
        if generation == 0 || paused.is_some() {
            return Err("view.external_busy".into());
        }
        self.external
            .compare_exchange(0, generation, Ordering::AcqRel, Ordering::Acquire)
            .map_err(|_| "view.external_busy".to_string())?;
        Ok(())
    }
    pub(crate) async fn wait_external_pause(&self, generation: u64) -> Result<()> {
        let mut wake = self.wake.subscribe();
        loop {
            {
                let paused = self.paused.lock().map_err(|_| "view.poisoned")?;
                self.ensure_open()?;
                if self.external.load(Ordering::Acquire) != generation {
                    return Err("view.stale_external".into());
                }
                if paused
                    .as_ref()
                    .is_some_and(|p| p.generation == generation && !p.resumed && p.native_inflight)
                {
                    return Ok(());
                }
            }
            wake.changed()
                .await
                .map_err(|_| "view.closed".to_string())?;
        }
    }
    pub(crate) fn external_checked(&self) -> bool {
        self.external.load(Ordering::Acquire) != 0 && self.native_checked()
    }

    pub(crate) fn begin_native(&self, generation: u64) -> Result<()> {
        let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        self.ensure_open()?;
        let p = paused
            .as_mut()
            .filter(|p| p.generation == generation && !p.resumed && !p.native_started)
            .ok_or("view.stale_native_cycle")?;
        p.native_started = true;
        p.native_inflight = true;
        Ok(())
    }
    pub(crate) fn finish_native(&self, generation: u64) -> Result<()> {
        let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        self.ensure_open()?;
        let p = paused
            .as_mut()
            .filter(|p| p.generation == generation && !p.resumed && p.native_inflight)
            .ok_or("view.stale_native_cycle")?;
        p.native_inflight = false;
        self.native_checked.store(true, Ordering::Release);
        Ok(())
    }
    pub(crate) fn native_checked(&self) -> bool {
        self.native_checked.load(Ordering::Acquire)
    }

    pub(crate) async fn wait_resume(&self, generation: u64) -> Result<()> {
        let mut wake = self.wake.subscribe();
        loop {
            let resumed = {
                let paused = self.paused.lock().map_err(|_| "view.poisoned")?;
                self.ensure_open()?;
                let p = paused
                    .as_ref()
                    .filter(|p| p.generation == generation)
                    .ok_or("view.unrequested_detach")?;
                p.resumed
            };
            if resumed {
                let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
                self.ensure_open()?;
                if paused
                    .as_ref()
                    .is_some_and(|p| p.generation == generation && p.resumed)
                {
                    *paused = None;
                    return Ok(());
                }
                return Err("view.stale_visibility".into());
            }
            wake.changed().await.map_err(|_| "view.closed")?;
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn destroy_wakes_paused_waiter_and_never_authorizes_late_resume() {
        let gate = VisibilityGate::default();
        assert!(gate.pause(2, || Ok(())).is_ok());
        let mut waiting = Box::pin(gate.wait_resume(2));
        assert!(futures_util::poll!(&mut waiting).is_pending());
        gate.close();
        assert!(
            tokio::time::timeout(std::time::Duration::from_millis(100), waiting)
                .await
                .is_ok_and(|r| r.is_err())
        );
        assert!(gate.resume(2).is_err());
        assert!(gate.pause(3, || Ok(())).is_err());
        assert!(gate.begin_native(2).is_err());
    }
    #[tokio::test]
    async fn destroy_before_wait_and_after_early_resume_cannot_reactivate_old_window() {
        for resumed in [false, true] {
            let gate = VisibilityGate::default();
            assert!(gate.pause(1, || Ok(())).is_ok());
            if resumed {
                assert!(gate.resume(1).is_ok());
            }
            gate.close();
            gate.close();
            assert!(gate.wait_resume(1).await.is_err());
            assert!(gate.hidden(1).is_err());
            assert!(gate.resume(1).is_err());
        }
    }
    #[tokio::test]
    async fn native_cycle_requires_pause_and_show_receipt_before_resume() {
        let gate = VisibilityGate::default();
        assert!(gate.begin_native(2).is_err());
        assert!(gate.finish_native(2).is_err());
        assert!(!gate.native_checked());
        assert!(gate.pause(2, || Ok(())).is_ok());
        assert!(gate.begin_native(1).is_err());
        assert!(gate.begin_native(2).is_ok());
        assert!(gate.begin_native(2).is_err());
        assert!(gate.resume(2).is_err());
        assert!(gate.finish_native(1).is_err());
        assert!(!gate.native_checked());
        assert!(gate.finish_native(2).is_ok());
        assert!(gate.native_checked());
        assert!(gate.finish_native(2).is_err());
        assert!(gate.begin_native(2).is_err());
        assert!(gate.resume(2).is_ok());
        assert!(gate.wait_resume(2).await.is_ok());
    }
    #[test]
    fn destroy_during_native_cycle_cannot_commit_visibility_receipt() {
        let gate = VisibilityGate::default();
        assert!(gate.pause(1, || Ok(())).is_ok());
        assert!(gate.begin_native(1).is_ok());
        gate.close();
        assert!(gate.finish_native(1).is_err());
        assert!(gate.resume(1).is_err());
        assert!(!gate.native_checked());
    }

    #[test]
    fn destruction_still_wakes_observers_after_metadata_poison() {
        let gate = VisibilityGate::default();
        let wake = gate.wake.subscribe();
        assert!(
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(
                || gate.pause(1, || panic!("synthetic revoke panic"))
            ))
            .is_err()
        );
        gate.close();
        assert!(gate.ensure_open().is_err());
        assert!(wake.has_changed().is_ok_and(|changed| changed));
        assert!(gate.resume(1).is_err());
    }
    #[tokio::test]
    async fn native_reservation_cannot_publish_pause_or_resume_by_itself() {
        let gate = VisibilityGate::default();
        assert!(gate.reserve_external(2).is_ok());
        assert!(gate.reserve_external(2).is_err());
        assert!(gate.reserve_external(3).is_err());
        assert!(gate.hidden(2).is_err());
        assert!(gate.resume(2).is_err());
        assert!(!gate.external_checked());
        let mut paused = Box::pin(gate.wait_external_pause(2));
        assert!(futures_util::poll!(&mut paused).is_pending());
        assert!(gate.pause(1, || Ok(())).is_err());
        assert!(gate.pause(2, || Ok(())).is_ok());
        assert!(paused.await.is_ok());
        assert!(gate.begin_native(2).is_err());
        assert!(gate.resume(2).is_err());
        assert!(gate.finish_native(2).is_ok());
        assert!(gate.external_checked());
        assert!(gate.resume(2).is_ok());
        assert!(gate.wait_resume(2).await.is_ok());
        assert!(gate.reserve_external(2).is_err());
    }
    #[tokio::test]
    async fn closing_unpaused_native_action_immediately_rejects_waiting_and_late_pause() {
        let gate = VisibilityGate::default();
        assert!(gate.reserve_external(1).is_ok());
        let mut wait = Box::pin(gate.wait_external_pause(1));
        assert!(futures_util::poll!(&mut wait).is_pending());
        gate.close();
        assert!(wait.await.is_err());
        assert!(gate.pause(1, || Ok(())).is_err());
        assert!(gate.finish_native(1).is_err());
        assert!(!gate.external_checked());
    }
    #[tokio::test]
    async fn external_pause_early_publication_and_failed_revoke_cannot_fabricate_receipts() {
        let gate = VisibilityGate::default();
        assert!(gate.reserve_external(4).is_ok());
        assert!(gate.pause(4, || Err("denied".into())).is_err());
        assert!(gate.hidden(4).is_err());
        assert!(gate.finish_native(4).is_err());
        assert!(gate.wait_external_pause(3).await.is_err());
        assert!(gate.pause(4, || Ok(())).is_ok());
        assert!(gate.wait_external_pause(4).await.is_ok());
        assert!(gate.finish_native(3).is_err());
        assert!(!gate.external_checked());
    }
    #[test]
    fn native_external_reservation_rejects_zero_or_already_paused_state() {
        let gate = VisibilityGate::default();
        assert!(gate.reserve_external(0).is_err());
        assert!(gate.pause(1, || Ok(())).is_ok());
        assert!(gate.reserve_external(1).is_err());
        assert!(gate.begin_native(1).is_ok());
        assert!(gate.finish_native(1).is_ok());
        assert!(!gate.external_checked());
    }

    #[test]
    fn invalid_or_failed_pause_cannot_authorize_resume() {
        let gate = VisibilityGate::default();
        assert!(gate.resume(1).is_err());
        assert!(gate.pause(1, || Err("denied".into())).is_err());
        assert!(gate.resume(1).is_err());
        assert!(gate.hidden(1).is_err());
    }
    #[tokio::test]
    async fn resume_is_generation_bound_single_use_and_does_not_lose_early_signal() {
        let gate = VisibilityGate::default();
        assert!(gate.pause(2, || Ok(())).is_ok());
        assert!(gate.pause(3, || Ok(())).is_err());
        assert!(gate.resume(1).is_err());
        assert!(gate.hidden(2).is_ok());
        assert!(gate.resume(2).is_ok());
        assert!(gate.resume(2).is_err());
        assert!(gate.hidden(2).is_err());
        assert!(gate.wait_resume(2).await.is_ok());
        assert!(gate.resume(2).is_err());
        assert!(gate.pause(3, || Ok(())).is_ok());
        assert!(gate.resume(2).is_err());
    }
    #[tokio::test]
    async fn only_explicit_resume_wakes_hidden_binding() {
        let gate = VisibilityGate::default();
        assert!(gate.pause(1, || Ok(())).is_ok());
        let mut wait = Box::pin(gate.wait_resume(1));
        tokio::select! {r=&mut wait=>panic!("premature resume {r:?}"),_=tokio::time::sleep(std::time::Duration::from_millis(10))=>{}}
        assert!(gate.resume(1).is_ok());
        assert!(wait.await.is_ok());
    }
}
