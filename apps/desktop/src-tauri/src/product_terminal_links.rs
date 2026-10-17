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

#[cfg(any(target_os = "linux", test))]
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
    #[cfg(not(target_os = "linux"))]
    {
        let _ = url;
        Err("workspace.link_unavailable")
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
