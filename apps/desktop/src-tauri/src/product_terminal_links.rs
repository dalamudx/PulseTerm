//! Explicit current-page HTTP(S) handoff; no shell, WebView navigation or URI logging.
use super::*;

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkTarget {
    url: String,
}

fn checked_target(raw: &str) -> Result<String> {
    if raw.is_empty()
        || raw.len() > 4096
        || raw.chars().any(|c| {
            c.is_control()
                || c.is_whitespace()
                || matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
        })
    {
        return Err("workspace.link_invalid");
    }
    let url = tauri::Url::parse(raw).map_err(|_| "workspace.link_invalid")?;
    if !matches!(url.scheme(), "http" | "https")
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.as_str() != raw
    {
        return Err("workspace.link_invalid");
    }
    Ok(url.into())
}

#[cfg(any(target_os = "linux", windows, target_os = "macos", test))]
impl TerminalBank {
    fn reserve_link(&self) -> Result<OwnedSemaphorePermit> {
        let credit = self
            .link_opens
            .clone()
            .try_acquire_owned()
            .map_err(|_| "workspace.busy")?;
        if self.closed.load(std::sync::atomic::Ordering::SeqCst) {
            return Err("workspace.denied");
        }
        Ok(credit)
    }
}

#[cfg(any(windows, target_os = "macos", test))]
fn complete_link_open(
    credit: OwnedSemaphorePermit,
    tx: oneshot::Sender<Result<()>>,
    native: impl FnOnce(&oneshot::Sender<Result<()>>) -> Result<()>,
) {
    let _credit = credit;
    let result = native(&tx);
    let _ = tx.send(result);
}

#[cfg(any(windows, test))]
fn spawn_link_open(
    credit: OwnedSemaphorePermit,
    native: impl FnOnce(&oneshot::Sender<Result<()>>) -> Result<()> + Send + 'static,
) -> Result<oneshot::Receiver<Result<()>>> {
    let (tx, rx) = oneshot::channel();
    std::thread::Builder::new()
        .name("web-link-handoff".into())
        .spawn(move || {
            complete_link_open(credit, tx, native);
        })
        .map_err(|_| "workspace.link_open_failed")?;
    Ok(rx)
}

#[tauri::command]
pub(crate) async fn workspace_terminal_link_open(
    window: WebviewWindow,
    state: tauri::State<'_, Workspace>,
    nonce: String,
    target: LinkTarget,
) -> Result<()> {
    page(&window, &state, &nonce)?;
    let url = checked_target(&target.url)?;
    #[cfg(target_os = "linux")]
    {
        let credit = state.0.terminals.reserve_link()?;
        let original = state.inner().clone();
        let source = window.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let (tx, rx) = oneshot::channel();
        window
            .with_webview(move |_| {
                if tx.is_closed()
                    || std::time::Instant::now() >= deadline
                    || original
                        .0
                        .terminals
                        .closed
                        .load(std::sync::atomic::Ordering::SeqCst)
                    || page(&source, &original, &nonce).is_err()
                {
                    let _ = tx.send(Err("workspace.denied"));
                    return;
                }
                gtk::gio::AppInfo::launch_default_for_uri_async(
                    &url,
                    None::<&gtk::gio::AppLaunchContext>,
                    None::<&gtk::gio::Cancellable>,
                    move |result| {
                        // Cancellation of the IPC waiter cannot release native I/O credit.
                        let _credit = credit;
                        let _ = tx.send(result.map_err(|_| "workspace.link_open_failed"));
                    },
                );
            })
            .map_err(|_| "workspace.link_open_failed")?;
        rx.await.map_err(|_| "workspace.link_open_failed")?
    }
    #[cfg(windows)]
    {
        let credit = state.0.terminals.reserve_link()?;
        let original = state.inner().clone();
        let source = window.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let rx = spawn_link_open(credit, move |tx| {
            pulse_platform::windows::open_web_url(&url, || {
                page(&source, &original, &nonce).is_ok()
                    && !tx.is_closed()
                    && std::time::Instant::now() < deadline
                    && !original
                        .0
                        .terminals
                        .closed
                        .load(std::sync::atomic::Ordering::SeqCst)
            })
            .map_err(|error| match error {
                pulse_platform::windows::WebLinkError::Denied => "workspace.denied",
                _ => "workspace.link_open_failed",
            })
        })?;
        rx.await.map_err(|_| "workspace.link_open_failed")?
    }
    #[cfg(target_os = "macos")]
    {
        let credit = state.0.terminals.reserve_link()?;
        let original = state.inner().clone();
        let source = window.clone();
        let deadline = std::time::Instant::now() + Duration::from_secs(3);
        let (tx, rx) = oneshot::channel();
        window
            .run_on_main_thread(move || {
                complete_link_open(credit, tx, |tx| {
                    pulse_platform::macos::open_web_url(&url, || {
                        page(&source, &original, &nonce).is_ok()
                            && !tx.is_closed()
                            && std::time::Instant::now() < deadline
                            && !original
                                .0
                                .terminals
                                .closed
                                .load(std::sync::atomic::Ordering::SeqCst)
                    })
                    .map_err(|error| match error {
                        pulse_platform::macos::WebLinkError::Denied => "workspace.denied",
                        _ => "workspace.link_open_failed",
                    })
                });
            })
            .map_err(|_| "workspace.link_open_failed")?;
        rx.await.map_err(|_| "workspace.link_open_failed")?
    }
    #[cfg(not(any(target_os = "linux", windows, target_os = "macos")))]
    {
        let _ = url;
        Err("workspace.link_unavailable")
    }
}

#[cfg(test)]
mod native_handoff_tests {
    use super::*;

    #[tokio::test]
    async fn queued_callback_retains_credit_until_actual_callback_completion()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let bank = Arc::new(TerminalBank::new());
        let credit = bank.reserve_link()?;
        let (tx, rx) = oneshot::channel();
        let queued = move || complete_link_open(credit, tx, |_| Err("workspace.denied"));
        drop(rx);
        assert!(bank.reserve_link().is_err());
        let closing = bank.clone();
        let mut shutdown = tokio::spawn(async move { closing.shutdown().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut shutdown)
                .await
                .is_err()
        );
        queued();
        tokio::time::timeout(Duration::from_secs(2), shutdown).await???;
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_waiter_retains_real_thread_credit_until_native_returns()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let bank = Arc::new(TerminalBank::new());
        let (entered_tx, entered_rx) = oneshot::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let rx = spawn_link_open(bank.reserve_link()?, move |_| {
            let _ = entered_tx.send(());
            release_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "test.release_missing")?;
            Ok(())
        })?;
        entered_rx.await?;
        drop(rx);
        assert!(bank.reserve_link().is_err());
        let closing = bank.clone();
        let mut shutdown = tokio::spawn(async move { closing.shutdown().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut shutdown)
                .await
                .is_err()
        );
        release_tx.send(())?;
        tokio::time::timeout(Duration::from_secs(2), shutdown).await???;
        assert!(bank.reserve_link().is_err());
        Ok(())
    }

    #[tokio::test]
    async fn cancelled_before_dispatch_does_not_call_handler()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let bank = Arc::new(TerminalBank::new());
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        let (done_tx, done_rx) = oneshot::channel();
        let rx = spawn_link_open(bank.reserve_link()?, move |tx| {
            release_rx
                .recv_timeout(Duration::from_secs(3))
                .map_err(|_| "test.release_missing")?;
            let cancelled = tx.is_closed();
            let _ = done_tx.send(cancelled);
            if cancelled {
                Err("workspace.denied")
            } else {
                Err("test.unexpected_dispatch")
            }
        })?;
        drop(rx);
        release_tx.send(())?;
        assert!(done_rx.await?);
        tokio::time::timeout(Duration::from_secs(2), bank.shutdown()).await??;
        Ok(())
    }

    #[tokio::test]
    async fn native_failure_is_reported_and_releases_credit()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let bank = Arc::new(TerminalBank::new());
        let rx = spawn_link_open(bank.reserve_link()?, |_| Err("workspace.link_open_failed"))?;
        assert_eq!(rx.await?, Err("workspace.link_open_failed"));
        // Shutdown waits for the actual permit release, not just the oneshot result.
        tokio::time::timeout(Duration::from_secs(2), bank.shutdown()).await??;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn link_open_accepts_only_bounded_canonical_http_targets() {
        for uri in [
            "https://example.test/",
            "http://[::1]:8080/a?q=x%20y#section",
            "https://xn--fsqu00a.test/%E4%B8%AD",
        ] {
            assert_eq!(checked_target(uri), Ok(uri.to_owned()));
        }
        for uri in [
            "",
            "javascript:alert(1)",
            "file:///etc/passwd",
            "mailto:a@example.test",
            "https://user:secret@example.test/",
            "https://example.test/\n",
            "https://example.test/\u{202e}a",
            "https://example.test",
            "https://EXAMPLE.test/",
            "https://example.test\\evil",
            "--help",
        ] {
            assert!(checked_target(uri).is_err());
        }
        assert!(checked_target(&format!("https://example.test/{}", "a".repeat(4096))).is_err());
        assert!(
            serde_json::from_value::<LinkTarget>(
                serde_json::json!({"url":"https://example.test/", "argv":["--help"]})
            )
            .is_err()
        );
        assert!(serde_json::from_value::<LinkTarget>(serde_json::json!({"url":false})).is_err());
    }

    #[tokio::test]
    async fn link_open_waiter_cancellation_retains_capacity_and_exit_barrier()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let bank = Arc::new(TerminalBank::new());
        let credit = bank.reserve_link()?;
        assert!(bank.reserve_link().is_err());
        let (tx, rx) = oneshot::channel::<()>();
        drop(rx);
        let waiting_bank = bank.clone();
        let mut shutdown = tokio::spawn(async move { waiting_bank.shutdown().await });
        assert!(
            tokio::time::timeout(Duration::from_millis(20), &mut shutdown)
                .await
                .is_err()
        );
        assert!(bank.reserve_link().is_err());
        drop(tx);
        drop(credit);
        tokio::time::timeout(Duration::from_secs(2), shutdown).await???;
        assert!(bank.reserve_link().is_err());
        Ok(())
    }
    #[test]
    fn link_open_rechecks_original_page_after_native_handoff()
    -> std::result::Result<(), Box<dyn std::error::Error>> {
        let registry = SessionRegistry::new();
        let docs = ProductDocuments::new(registry.clone());
        let owner = registry.open_window()?;
        let other = registry.open_window()?;
        docs.install(owner)?;
        docs.install(other)?;
        docs.started(owner)?;
        let nonce = docs.finished(owner)?;
        docs.confirm(owner, nonce)?;
        let source = tauri::Url::parse("tauri://localhost/workspace.html")?;
        assert!(current_document(&docs, owner, &source, &nonce.to_string()).is_ok());
        assert!(current_document(&docs, other, &source, &nonce.to_string()).is_err());
        assert!(
            current_document(
                &docs,
                owner,
                &tauri::Url::parse("https://example.test/")?,
                &nonce.to_string()
            )
            .is_err()
        );
        docs.started(owner)?;
        assert!(current_document(&docs, owner, &source, &nonce.to_string()).is_err());
        let new_nonce = docs.finished(owner)?;
        docs.confirm(owner, new_nonce)?;
        assert!(current_document(&docs, owner, &source, &nonce.to_string()).is_err());
        assert!(current_document(&docs, owner, &source, &new_nonce.to_string()).is_ok());
        Ok(())
    }
}
