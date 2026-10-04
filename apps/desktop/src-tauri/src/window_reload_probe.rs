//! Real fixed peer WebView reload with Rust-only recapture. No production navigation hook.
use super::*;
use tauri::webview::PageLoadEvent;
use tokio::sync::watch;
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
struct Pages {
    started: u8,
    finished: u8,
    invalid: bool,
}
#[derive(Clone)]
pub(super) struct PageTracker(watch::Sender<Pages>);
impl PageTracker {
    pub(super) fn new() -> Self {
        let (tx, _) = watch::channel(Pages::default());
        Self(tx)
    }
    pub(super) fn record(&self, event: PageLoadEvent) {
        self.0.send_modify(|p| {
            if p.invalid {
                return;
            }
            match event {
                PageLoadEvent::Started => {
                    if p.started != p.finished || p.started >= 2 {
                        p.invalid = true;
                    } else {
                        p.started += 1;
                    }
                }
                PageLoadEvent::Finished => {
                    if p.started != p.finished + 1 {
                        p.invalid = true;
                    } else {
                        p.finished += 1;
                    }
                }
            }
        });
    }
    fn snapshot(&self) -> Pages {
        *self.0.borrow()
    }
}
fn require(value: bool) -> Result<()> {
    if value {
        Ok(())
    } else {
        Err("session.peer_reload_invalid".into())
    }
}
pub(super) async fn verify(
    state: &WindowSessions,
    window: &WebviewWindow,
    owner: WindowToken,
    id: SessionId,
    handle: &SessionHandle,
    pages: &PageTracker,
) -> Result<()> {
    require(
        pages.snapshot()
            == Pages {
                started: 1,
                finished: 1,
                invalid: false,
            }
            && state.caller(window)? == owner,
    )?;
    let old = transfer::capture(state, owner, id, handle).await?;
    let before = state.0.registry.state(owner, id).map_err(error)?;
    // Peer JS has never been granted access to this session: its only script probes
    // forbidden main-session controls. No page token/nonce reuse is authorized here.
    // Production adapters must revoke old page nonce/epoch BEFORE this registry step.
    require(state.0.registry.retire_window_views(owner).map_err(error)? == 1)?;
    require(
        state
            .0
            .registry
            .acknowledge(owner, id, old.generation, 0)
            .is_err()
            && state
                .0
                .registry
                .input_bound(owner, id, old.generation, b"denied\n".to_vec())
                .await
                .is_err(),
    )?;
    let mut changed = pages.0.subscribe();
    eprintln!("session.peer_reload.requested");
    window.reload().map_err(error)?;
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let p = *changed.borrow();
            require(!p.invalid)?;
            if p == (Pages {
                started: 2,
                finished: 2,
                invalid: false,
            }) {
                return Ok::<(), String>(());
            }
            changed.changed().await.map_err(error)?;
        }
    })
    .await
    .map_err(|_| "session.peer_reload_page_deadline".to_string())??;
    require(
        state.caller(window)? == owner
            && state.0.registry.state(owner, id).map_err(error)? == before
            && handle.state.borrow().lifecycle == Lifecycle::Running
            && window
                .app_handle()
                .get_webview_window(window.label())
                .is_some(),
    )?;
    let fresh = transfer::capture(state, owner, id, handle).await?;
    require(fresh.generation > old.generation)?;
    let old_result = tokio::time::timeout(Duration::from_secs(3), old.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        old_result,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    state
        .0
        .registry
        .check_binding(owner, id, fresh.generation)
        .map_err(error)?;
    state
        .0
        .registry
        .detach(owner, id, fresh.generation)
        .map_err(error)?;
    let result = tokio::time::timeout(Duration::from_secs(3), fresh.completion)
        .await
        .map_err(error)?
        .map_err(error)?;
    require(matches!(
        result,
        Err(pulse_app::attachment::AttachmentError::Detached)
    ))?;
    eprintln!("session.peer_reload.recaptured");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn two_real_page_cycles_are_required_and_early_finish_is_not_ready() {
        let p = PageTracker::new();
        p.record(PageLoadEvent::Started);
        assert_eq!(p.snapshot().finished, 0);
        p.record(PageLoadEvent::Finished);
        assert_eq!(
            p.snapshot(),
            Pages {
                started: 1,
                finished: 1,
                invalid: false
            }
        );
        p.record(PageLoadEvent::Started);
        assert_eq!(p.snapshot().finished, 1);
        p.record(PageLoadEvent::Finished);
        assert_eq!(
            p.snapshot(),
            Pages {
                started: 2,
                finished: 2,
                invalid: false
            }
        );
        p.record(PageLoadEvent::Started);
        assert!(p.snapshot().invalid);
    }
    #[test]
    fn duplicated_or_out_of_order_callbacks_never_fabricate_fresh_page() {
        for bad in [
            vec![PageLoadEvent::Finished],
            vec![PageLoadEvent::Started, PageLoadEvent::Started],
            vec![
                PageLoadEvent::Started,
                PageLoadEvent::Finished,
                PageLoadEvent::Finished,
            ],
        ] {
            let p = PageTracker::new();
            for e in bad {
                p.record(e);
            }
            assert!(p.snapshot().invalid);
            p.record(PageLoadEvent::Started);
            p.record(PageLoadEvent::Finished);
            assert!(p.snapshot().invalid);
            assert!(p.snapshot().finished < 2);
        }
    }
}
