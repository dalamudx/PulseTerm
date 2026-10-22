//! Linux validation only: a blank page never receives production document authority.
use super::*;
use std::ffi::OsStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Page {
    Workspace,
    Blank,
}
impl Page {
    pub(super) fn selected() -> Result<Self> {
        Self::parse(std::env::var_os("PULSE_RUNTIME_MEMORY_PAGE").as_deref())
    }
    fn parse(value: Option<&OsStr>) -> Result<Self> {
        match value.and_then(OsStr::to_str) {
            None if value.is_none() => Ok(Self::Workspace),
            Some("workspace") => Ok(Self::Workspace),
            Some("blank") => Ok(Self::Blank),
            _ => Err("workspace.runtime_memory_mode_invalid"),
        }
    }
    pub(super) fn url(self) -> Result<WebviewUrl> {
        match self {
            Self::Workspace => Ok(WebviewUrl::App("workspace.html".into())),
            Self::Blank => tauri::Url::parse("about:blank")
                .map(WebviewUrl::External)
                .map_err(|_| "workspace.runtime_memory_url_invalid"),
        }
    }
}
fn blank_source(label: &str, payload: &tauri::Url, current: &tauri::Url) -> Result<()> {
    if label != LABEL || payload.as_str() != "about:blank" || current.as_str() != "about:blank" {
        return Err("workspace.denied");
    }
    Ok(())
}
pub(super) fn blank_load(
    state: &Workspace,
    window: &WebviewWindow,
    event: PageLoadEvent,
    url: &tauri::Url,
) -> Result<()> {
    blank_source(
        window.label(),
        url,
        &window.url().map_err(|_| "workspace.denied")?,
    )?;
    match event {
        // Keep navigation revocation, but never call documents.finished or inject a nonce.
        PageLoadEvent::Started => state.on_page_load(window, event, url),
        PageLoadEvent::Finished => {
            eprintln!("workspace.runtime_memory.page.blank");
            Ok(())
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn mode_is_closed_and_default_is_the_workspace() {
        assert_eq!(Page::parse(None).unwrap(), Page::Workspace);
        assert_eq!(
            Page::parse(Some(OsStr::new("workspace"))).unwrap(),
            Page::Workspace
        );
        assert_eq!(Page::parse(Some(OsStr::new("blank"))).unwrap(), Page::Blank);
        for value in ["", "1", "Blank", "about:blank", "blank "] {
            assert!(Page::parse(Some(OsStr::new(value))).is_err());
        }
        use std::os::unix::ffi::OsStrExt;
        assert!(Page::parse(Some(OsStr::from_bytes(&[0xff]))).is_err());
    }
    #[test]
    fn blank_requires_exact_label_and_both_exact_urls() {
        let blank = tauri::Url::parse("about:blank").unwrap();
        blank_source(LABEL, &blank, &blank).unwrap();
        assert!(blank_source("foreign", &blank, &blank).is_err());
        for value in [
            "about:blank#x",
            "data:text/html,blank",
            "https://example.com",
            "http://tauri.localhost/workspace.html",
        ] {
            let other = tauri::Url::parse(value).unwrap();
            assert!(blank_source(LABEL, &other, &blank).is_err());
            assert!(blank_source(LABEL, &blank, &other).is_err());
        }
    }
}
