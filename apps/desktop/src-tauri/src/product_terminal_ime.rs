//! Keep IBus preedit inside WebKit; leave wry's Fcitx workaround intact.
use std::ffi::OsStr;

fn uses_ibus(environment: Option<&OsStr>, desktop: Option<&str>) -> bool {
    match environment.filter(|module| !module.is_empty()) {
        Some(module) => module == OsStr::new("ibus"),
        None => desktop == Some("ibus"),
    }
}

pub(super) fn install(window: &tauri::WebviewWindow) -> super::Result<()> {
    use gtk::prelude::GtkSettingsExt;
    use webkit2gtk::{InputMethodContextExt, WebViewExt};
    // with_webview runs on the GTK thread, after wry has configured the context.
    window
        .with_webview(|native| {
            let environment = std::env::var_os("GTK_IM_MODULE");
            let desktop = gtk::Settings::default().and_then(|settings| settings.gtk_im_module());
            if !uses_ibus(environment.as_deref(), desktop.as_deref()) {
                return;
            }
            if let Some(context) = native.inner().input_method_context() {
                // wry 0.57 disables preedit globally for Fcitx. IBus needs it enabled.
                context.set_enable_preedit(true);
            }
        })
        .map_err(|_| "workspace.window_unconfirmed")
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn explicit_ibus_takes_precedence() {
        assert!(uses_ibus(Some(OsStr::new("ibus")), Some("fcitx")));
        assert!(uses_ibus(Some(OsStr::new("ibus")), None));
    }
    #[test]
    fn other_explicit_modules_keep_the_existing_behavior() {
        for module in ["fcitx", "fcitx5", "xim", "simple", "ibus:xim", "unknown"] {
            assert!(!uses_ibus(Some(OsStr::new(module)), Some("ibus")));
        }
    }
    #[test]
    fn desktop_selection_needs_no_environment_override() {
        assert!(uses_ibus(None, Some("ibus")));
        assert!(uses_ibus(Some(OsStr::new("")), Some("ibus")));
        for module in [None, Some("fcitx"), Some("simple"), Some("unknown")] {
            assert!(!uses_ibus(None, module));
        }
    }
    #[test]
    fn invalid_environment_does_not_fall_back_to_ibus() {
        use std::os::unix::ffi::OsStrExt;
        assert!(!uses_ibus(Some(OsStr::from_bytes(&[0xff])), Some("ibus")));
    }
}
