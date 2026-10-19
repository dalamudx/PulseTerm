//! Keep preedit inside WebKit for the explicitly selected GTK input modules.
use std::ffi::OsStr;

fn uses_client_preedit(environment: Option<&OsStr>, desktop: Option<&str>) -> bool {
    match environment.filter(|module| !module.is_empty()) {
        Some(module) => module == OsStr::new("ibus") || module == OsStr::new("fcitx"),
        None => matches!(desktop, Some("ibus" | "fcitx")),
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
            if !uses_client_preedit(environment.as_deref(), desktop.as_deref()) {
                return;
            }
            if let Some(context) = native.inner().input_method_context() {
                // wry's blanket preedit disablement leaves the candidate cursor stale.
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
        assert!(uses_client_preedit(
            Some(OsStr::new("ibus")),
            Some("unknown")
        ));
        assert!(uses_client_preedit(Some(OsStr::new("ibus")), None));
    }
    #[test]
    fn explicit_fcitx_takes_precedence() {
        assert!(uses_client_preedit(
            Some(OsStr::new("fcitx")),
            Some("unknown")
        ));
        assert!(uses_client_preedit(Some(OsStr::new("fcitx")), None));
    }
    #[test]
    fn other_explicit_modules_keep_the_existing_behavior() {
        for module in ["fcitx5", "xim", "simple", "ibus:xim", "unknown"] {
            for desktop in ["ibus", "fcitx"] {
                assert!(!uses_client_preedit(
                    Some(OsStr::new(module)),
                    Some(desktop)
                ));
            }
        }
    }
    #[test]
    fn desktop_selection_needs_no_environment_override() {
        for module in ["ibus", "fcitx"] {
            assert!(uses_client_preedit(None, Some(module)));
            assert!(uses_client_preedit(Some(OsStr::new("")), Some(module)));
        }
        for module in [None, Some("fcitx5"), Some("simple"), Some("unknown")] {
            assert!(!uses_client_preedit(None, module));
        }
    }
    #[test]
    fn invalid_environment_does_not_fall_back_to_desktop() {
        use std::os::unix::ffi::OsStrExt;
        for module in ["ibus", "fcitx"] {
            assert!(!uses_client_preedit(
                Some(OsStr::from_bytes(&[0xff])),
                Some(module)
            ));
        }
    }
}
