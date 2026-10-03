//! Finite viewer lifecycle metadata. No terminal lock or frontend owner token.
use std::sync::Mutex;
use tokio::sync::watch;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, Copy)]
struct Pause {
    generation: u64,
    resumed: bool,
}
pub(crate) struct VisibilityGate {
    paused: Mutex<Option<Pause>>,
    wake: watch::Sender<u64>,
}
impl Default for VisibilityGate {
    fn default() -> Self {
        let (wake, _) = watch::channel(0);
        Self {
            paused: Mutex::new(None),
            wake,
        }
    }
}
impl VisibilityGate {
    pub(crate) fn pause(&self, generation: u64, revoke: impl FnOnce() -> Result<()>) -> Result<()> {
        let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
        if paused.is_some() {
            return Err("view.already_paused".into());
        }
        revoke()?;
        *paused = Some(Pause {
            generation,
            resumed: false,
        });
        Ok(())
    }
    pub(crate) fn hidden(&self, generation: u64) -> Result<()> {
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
        let p = paused
            .as_mut()
            .filter(|p| p.generation == generation && !p.resumed)
            .ok_or("view.stale_visibility")?;
        p.resumed = true;
        self.wake.send_replace(generation);
        Ok(())
    }
    pub(crate) async fn wait_resume(&self, generation: u64) -> Result<()> {
        let mut wake = self.wake.subscribe();
        loop {
            let resumed = {
                let paused = self.paused.lock().map_err(|_| "view.poisoned")?;
                let p = paused
                    .as_ref()
                    .filter(|p| p.generation == generation)
                    .ok_or("view.unrequested_detach")?;
                p.resumed
            };
            if resumed {
                let mut paused = self.paused.lock().map_err(|_| "view.poisoned")?;
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
