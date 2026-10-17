//! Shared native origin check for local bundled assets. An origin alone never
//! authorizes a document, window, Profile, input, or session operation.

/// Accept only the Tauri application asset origins, not dev servers, remote pages,
/// credentials, or custom ports. Callers must separately verify the WindowToken
/// and current page generation before granting state-changing permissions.
pub(crate) fn local_url(url: &tauri::Url) -> bool {
    url.username().is_empty()
        && url.password().is_none()
        && url.port().is_none()
        && matches!(
            (url.scheme(), url.host_str()),
            ("tauri", Some("localhost")) | ("http" | "https", Some("tauri.localhost"))
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_only_tauri_app_asset_origins() -> Result<(), Box<dyn std::error::Error>> {
        for url in [
            "tauri://localhost/index.html",
            "http://tauri.localhost/index.html?session-peer=1",
            "https://tauri.localhost/",
        ] {
            assert!(local_url(&tauri::Url::parse(url)?));
        }
        for url in [
            "https://example.com/",
            "http://localhost/",
            "http://tauri.localhost:99/",
            "http://u@tauri.localhost/",
            "file:///tmp/index.html",
            "tauri://localhost.evil/",
            "data:text/html,hello",
        ] {
            assert!(!local_url(&tauri::Url::parse(url)?));
        }
        Ok(())
    }
}
