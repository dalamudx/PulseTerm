//! Event-hinted native sampling. Focus is a hint, NEVER a visibility decision.
use std::{
    sync::{Arc, Mutex},
    time::Duration,
};
use tauri::WebviewWindow;
use tauri_specta::Event;
use tokio::sync::watch;
type Result<T> = std::result::Result<T, String>;
#[derive(Clone, serde::Serialize, serde::Deserialize, specta::Type, tauri_specta::Event)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct NativeVisibilitySample {
    pub incarnation: String,
    pub revision: u32,
    pub visible: bool,
    pub minimized: bool,
    pub valid: bool,
}
struct Ledger {
    sample: NativeVisibilitySample,
    closed: bool,
}
pub(crate) struct NativeWatcher {
    ledger: Mutex<Ledger>,
    wake: watch::Sender<()>,
    query: Arc<tokio::sync::Semaphore>,
}
impl NativeWatcher {
    pub(crate) fn new() -> Arc<Self> {
        let (wake, _) = watch::channel(());
        Arc::new(Self {
            wake,
            query: Arc::new(tokio::sync::Semaphore::new(1)),
            ledger: Mutex::new(Ledger {
                closed: false,
                sample: NativeVisibilitySample {
                    incarnation: uuid::Uuid::new_v4().to_string(),
                    revision: 0,
                    visible: false,
                    minimized: false,
                    valid: false,
                },
            }),
        })
    }
    pub(crate) fn hint(&self) {
        self.wake.send_replace(());
    }
    pub(crate) fn close(&self) {
        self.ledger
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .closed = true;
        self.hint();
    }
    fn closed(&self) -> bool {
        self.ledger.lock().map_or(true, |l| l.closed)
    }
    fn commit(&self, state: Option<(bool, bool)>) -> Result<Option<NativeVisibilitySample>> {
        let mut ledger = self.ledger.lock().map_err(|_| "native.poisoned")?;
        if ledger.closed {
            return Err("native.closed".into());
        }
        let (visible, minimized, valid) =
            state.map_or((false, false, false), |(v, m)| (v, m, true));
        let old = &ledger.sample;
        if old.revision > 0
            && old.visible == visible
            && old.minimized == minimized
            && old.valid == valid
        {
            return Ok(None);
        }
        let revision = old
            .revision
            .checked_add(1)
            .ok_or("native.revision_exhausted")?;
        ledger.sample = NativeVisibilitySample {
            revision,
            visible,
            minimized,
            valid,
            incarnation: old.incarnation.clone(),
        };
        Ok(Some(ledger.sample.clone()))
    }
    async fn query(
        &self,
        read: impl FnOnce() -> Option<(bool, bool)> + Send + 'static,
    ) -> Result<Option<(bool, bool)>> {
        let permit = tokio::time::timeout(
            Duration::from_millis(500),
            self.query.clone().acquire_owned(),
        )
        .await
        .map_err(|_| "native.busy".to_string())?
        .map_err(|_| "native.closed".to_string())?;
        if self.closed() {
            return Err("native.closed".into());
        }
        // A timeout does not stop spawn_blocking. Its owned permit MUST remain
        // inside the actual job until it returns, not in the async caller.
        let query = tokio::task::spawn_blocking(move || {
            let measured = read();
            drop(permit);
            measured
        });
        Ok(
            match tokio::time::timeout(Duration::from_millis(500), query).await {
                Ok(Ok(value)) => value,
                _ => None,
            },
        )
    }
    pub(crate) async fn sample(&self, window: WebviewWindow) -> Result<NativeVisibilitySample> {
        let state = self
            .query(move || {
                window
                    .is_visible()
                    .and_then(|v| window.is_minimized().map(|m| (v, m)))
                    .ok()
            })
            .await?;
        self.commit(state)?;
        self.ledger
            .lock()
            .map_err(|_| "native.poisoned".into())
            .and_then(|l| {
                if l.closed {
                    Err("native.closed".into())
                } else {
                    Ok(l.sample.clone())
                }
            })
    }
    pub(crate) fn launch(self: &Arc<Self>, window: WebviewWindow) {
        let mut wake = self.wake.subscribe();
        let observer = self.clone();
        tauri::async_runtime::spawn(async move {
            let mut first = true;
            let mut last_emitted = 0;
            loop {
                if !first && wake.changed().await.is_err() {
                    break;
                }
                first = false;
                // Coalesce the latest window hints and sample only a bounded settling burst.
                // Desktop WindowEvent has no universal minimized event; no idle polling.
                for delay in [50, 50, 100] {
                    tokio::time::sleep(Duration::from_millis(delay)).await;
                    if observer.closed() {
                        return;
                    }
                    let sample = match observer.sample(window.clone()).await {
                        Ok(sample) => sample,
                        Err(_) => {
                            if let Ok(Some(invalid)) = observer.commit(None) {
                                let _ = invalid.emit_to(
                                    &window,
                                    tauri::EventTarget::webview_window(window.label()),
                                );
                            }
                            observer.close();
                            return;
                        }
                    };
                    if observer.closed() {
                        return;
                    }
                    let valid = sample.valid;
                    if sample.revision <= last_emitted {
                        continue;
                    }
                    last_emitted = sample.revision;
                    if sample
                        .emit_to(&window, tauri::EventTarget::webview_window(window.label()))
                        .is_err()
                    {
                        observer.close();
                        return;
                    }
                    if !valid {
                        observer.close();
                        return;
                    }
                }
            }
        });
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn timed_out_blocking_query_holds_credit_until_actual_completion() {
        let w = NativeWatcher::new();
        let (release, receive) = std::sync::mpsc::channel();
        assert!(
            w.query(move || {
                let _ = receive.recv_timeout(Duration::from_secs(3));
                Some((true, false))
            })
            .await
            .is_ok_and(|v| v.is_none())
        );
        assert_eq!(w.query.available_permits(), 0);
        let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let flag = called.clone();
        assert!(
            w.query(move || {
                flag.store(true, std::sync::atomic::Ordering::Relaxed);
                Some((true, false))
            })
            .await
            .is_err()
        );
        assert!(!called.load(std::sync::atomic::Ordering::Relaxed));
        assert!(release.send(()).is_ok());
        let returned =
            tokio::time::timeout(Duration::from_secs(1), w.query.clone().acquire_owned()).await;
        assert!(returned.is_ok_and(|permit| permit.is_ok()));
    }

    #[test]
    fn focus_hint_alone_never_changes_projection_and_same_sample_keeps_revision() {
        let w = NativeWatcher::new();
        assert!(w.commit(Some((true, false))).is_ok_and(|s| s.is_some()));
        w.hint();
        assert!(w.commit(Some((true, false))).is_ok_and(|s| s.is_none()));
        assert!(
            w.commit(Some((true, true)))
                .is_ok_and(|s| s.is_some_and(|s| s.revision == 2 && s.minimized))
        );
    }
    #[test]
    fn close_rejects_late_results_and_does_not_reopen_old_incarnation() {
        let w = NativeWatcher::new();
        assert!(w.commit(Some((true, false))).is_ok());
        w.close();
        assert!(w.commit(Some((false, true))).is_err());
        assert!(w.commit(Some((true, false))).is_err());
    }
    #[test]
    fn query_failure_is_invalid_not_visible_or_minimized_evidence() {
        let w = NativeWatcher::new();
        assert!(w.commit(Some((true, false))).is_ok());
        assert!(w.commit(None).is_ok_and(|s| {
            s.is_some_and(|s| !s.valid && !s.visible && !s.minimized && s.revision == 2)
        }));
    }
    #[test]
    fn observer_identity_is_per_window_incarnation() {
        let a = NativeWatcher::new();
        let b = NativeWatcher::new();
        let a = a.commit(Some((true, false)));
        let b = b.commit(Some((true, false)));
        assert!(matches!((a,b),(Ok(Some(a)),Ok(Some(b)))if a.incarnation!=b.incarnation));
    }
    #[test]
    fn revision_exhaustion_is_an_error_not_wrapping_into_stale_identity() {
        let w = NativeWatcher::new();
        if let Ok(mut l) = w.ledger.lock() {
            l.sample.revision = u32::MAX;
        }
        assert!(w.commit(Some((true, false))).is_err());
    }
    #[tokio::test]
    async fn hint_mailbox_coalesces_and_closed_wakes_waiter() {
        let w = NativeWatcher::new();
        let mut rx = w.wake.subscribe();
        for _ in 0..1000 {
            w.hint();
        }
        assert!(rx.changed().await.is_ok());
        assert!(!rx.has_changed().unwrap_or(true));
        w.close();
        assert!(rx.changed().await.is_ok());
        assert!(w.closed());
    }
}
