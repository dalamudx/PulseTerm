//! Restricted macOS system metadata and web handoff; no page, file or executable authority.
mod local_control;
pub use crate::web_link::WebLinkError;
pub use local_control::local_control_root;
use objc2::MainThreadMarker;
use objc2_app_kit::NSWorkspace;
use objc2_foundation::{NSString, NSURL};

/// Hand a caller-validated HTTP(S) URL to the system default handler on the main thread.
/// Authorization is checked after native URL construction, immediately before dispatch.
/// The caller must retain native I/O capacity until this synchronous handoff returns.
/// Success does not establish browser display, network completion or browser process exit.
pub fn open_web_url(url: &str, authorize: impl FnOnce() -> bool) -> Result<(), WebLinkError> {
    crate::web_link::validate(url)?;
    let _main = MainThreadMarker::new().ok_or(WebLinkError::Failed)?;
    let native_url = NSURL::URLWithString(&NSString::from_str(url)).ok_or(WebLinkError::Invalid)?;
    if native_url
        .absoluteString()
        .is_none_or(|value| value.to_string() != url)
    {
        return Err(WebLinkError::Invalid);
    }
    let workspace = NSWorkspace::sharedWorkspace();
    if !authorize() {
        return Err(WebLinkError::Denied);
    }
    if !workspace.openURL(&native_url) {
        return Err(WebLinkError::Failed);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_target_never_reaches_native_authorization() {
        assert_eq!(
            open_web_url("file:///etc/passwd", || panic!(
                "unexpected native authorization"
            )),
            Err(WebLinkError::Invalid)
        );
    }

    #[test]
    fn off_main_thread_never_reaches_native_authorization() {
        let result = std::thread::spawn(|| {
            open_web_url("https://example.test/", || {
                panic!("unexpected native authorization")
            })
        })
        .join();
        assert_eq!(result.ok(), Some(Err(WebLinkError::Failed)));
    }
}
