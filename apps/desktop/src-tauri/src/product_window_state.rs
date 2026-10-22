//! Owned per-WebView command state. Native callers install contexts; JSON cannot pick one.
//! This metadata map is NOT the native window-capacity or resource-drain controller.
use super::{Result, local_workspace_url};
use pulse_app::window_directory::WindowLabel;
use std::{
    collections::HashMap,
    marker::PhantomData,
    ops::Deref,
    sync::{Arc, Mutex},
};
use tauri::{
    Manager, Runtime,
    ipc::{CommandArg, CommandItem, InvokeError},
};

struct Entries<T> {
    contexts: HashMap<WindowLabel, T>,
    primary_used: bool,
}
pub(super) struct Contexts<T>(Arc<Mutex<Entries<T>>>);
impl<T> Clone for Contexts<T> {
    fn clone(&self) -> Self {
        Self(self.0.clone())
    }
}
impl<T: Clone> Contexts<T> {
    pub(super) fn new() -> Self {
        Self(Arc::new(Mutex::new(Entries {
            contexts: HashMap::new(),
            primary_used: false,
        })))
    }
    /// Native-only metadata installation. Physical creation and capacity belong elsewhere.
    pub(super) fn install(&self, label: &str, context: T) -> Result<()> {
        let label = WindowLabel::parse(label).map_err(|_| "workspace.denied")?;
        let mut entries = self.0.lock().map_err(|_| "workspace.denied")?;
        if entries.contexts.len() >= 32
            || entries.contexts.contains_key(&label)
            || (label.as_str() == super::LABEL && entries.primary_used)
        {
            return Err("workspace.denied");
        }
        entries.primary_used |= label.as_str() == super::LABEL;
        entries.contexts.insert(label, context);
        Ok(())
    }
    fn for_native_webview(&self, label: &str, url: &tauri::Url) -> Result<T> {
        if !local_workspace_url(url) {
            return Err("workspace.denied");
        }
        let label = WindowLabel::parse(label).map_err(|_| "workspace.denied")?;
        self.0
            .lock()
            .map_err(|_| "workspace.denied")?
            .contexts
            .get(&label)
            .cloned()
            .ok_or("workspace.denied")
    }
    // Native-only metadata routing. Callers still recheck directory/page/source authority.
    #[cfg(all(feature = "workspace-cli", any(target_os = "linux", windows)))]
    pub(super) fn for_native_label(&self, label: &WindowLabel) -> Result<T> {
        self.0
            .lock()
            .map_err(|_| "workspace.denied")?
            .contexts
            .get(label)
            .cloned()
            .ok_or("workspace.denied")
    }
    /// Stop new metadata extraction; this is not proof of physical resource completion.
    pub(super) fn retire(&self, label: &str) -> Result<()> {
        let label = WindowLabel::parse(label).map_err(|_| "workspace.denied")?;
        self.0
            .lock()
            .map_err(|_| "workspace.denied")?
            .contexts
            .remove(&label)
            .map(|_| ())
            .ok_or("workspace.denied")
    }
}
/// Owns its context across await. The lifetime marker preserves existing command signatures.
pub(super) struct State<'a, T> {
    inner: T,
    marker: PhantomData<&'a ()>,
}
impl<T> State<'_, T> {
    /// Borrow only this Invoke WebView's owned context, never the application default.
    pub(super) fn inner(&self) -> &T {
        &self.inner
    }
}
impl<T> Deref for State<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<'de, 'a, T: Clone + Send + Sync + 'static, R: Runtime> CommandArg<'de, R> for State<'a, T> {
    fn from_command(command: CommandItem<'de, R>) -> std::result::Result<Self, InvokeError> {
        let webview = command.message.webview_ref();
        let url = webview
            .url()
            .map_err(|_| InvokeError::from("workspace.denied"))?;
        let contexts = webview
            .try_state::<Contexts<T>>()
            .ok_or_else(|| InvokeError::from("workspace.denied"))?;
        let inner = contexts
            .for_native_webview(webview.label(), &url)
            .map_err(InvokeError::from)?;
        Ok(Self {
            inner,
            marker: PhantomData,
        })
    }
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    fn local() -> tauri::Url {
        tauri::Url::parse("tauri://localhost/workspace.html").unwrap()
    }
    #[test]
    fn exact_native_labels_choose_independent_contexts_without_default_fallback() {
        let contexts = Contexts::new();
        let other = format!("workspace-{}", uuid::Uuid::new_v4());
        contexts.install("workspace", "first").unwrap();
        contexts.install(&other, "second").unwrap();
        assert_eq!(
            contexts.for_native_webview("workspace", &local()).unwrap(),
            "first"
        );
        assert_eq!(
            contexts.for_native_webview(&other, &local()).unwrap(),
            "second"
        );
        let absent = format!("workspace-{}", uuid::Uuid::new_v4());
        assert!(contexts.for_native_webview(&absent, &local()).is_err());
        assert!(contexts.for_native_webview("", &local()).is_err());
    }
    #[test]
    fn remote_wrong_page_query_and_fragment_cannot_extract_installed_state() {
        let contexts = Contexts::new();
        contexts.install("workspace", "first").unwrap();
        for url in [
            "https://example.org/workspace.html",
            "tauri://localhost/probe.html",
            "tauri://localhost/workspace.html?window=workspace",
            "tauri://localhost/workspace.html#workspace",
        ] {
            assert!(
                contexts
                    .for_native_webview("workspace", &tauri::Url::parse(url).unwrap())
                    .is_err()
            );
        }
        assert!(contexts.install("workspace\n", "foreign").is_err());
    }
    #[test]
    fn duplicate_install_cannot_replace_context_and_closed_primary_is_not_reused() {
        let contexts = Contexts::new();
        contexts.install("workspace", "first").unwrap();
        let captured = contexts.for_native_webview("workspace", &local()).unwrap();
        assert!(contexts.install("workspace", "replacement").is_err());
        assert_eq!(
            contexts.for_native_webview("workspace", &local()).unwrap(),
            "first"
        );
        contexts.retire("workspace").unwrap();
        assert!(contexts.for_native_webview("workspace", &local()).is_err());
        assert!(contexts.install("workspace", "replacement").is_err());
        assert_eq!(captured, "first"); // Native effects still need original page/epoch revalidation.
    }
    #[test]
    fn retiring_one_context_preserves_the_other_and_metadata_storage_is_bounded() {
        let contexts = Contexts::new();
        contexts.install("workspace", 0).unwrap();
        let labels: Vec<_> = (0..31)
            .map(|_| format!("workspace-{}", uuid::Uuid::new_v4()))
            .collect();
        for (index, label) in labels.iter().enumerate() {
            contexts.install(label, index + 1).unwrap();
        }
        assert!(
            contexts
                .install(&format!("workspace-{}", uuid::Uuid::new_v4()), 99)
                .is_err()
        );
        contexts.retire("workspace").unwrap();
        assert_eq!(
            contexts.for_native_webview(&labels[0], &local()).unwrap(),
            1
        );
        assert!(contexts.retire("workspace").is_err());
    }
}
