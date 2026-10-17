//! Current-window visibility projection only; no terminal or window-control permission.
use super::*;
use tauri::Emitter;
const EVENT: &str = "workspace-native-visibility";

pub(super) fn install(window: &WebviewWindow, state: &Workspace) -> Result<()> {
    let observer = state.0.visibility.clone();
    #[cfg(target_os = "linux")]
    {
        use gtk::prelude::*;
        let native = window
            .gtk_window()
            .map_err(|_| "workspace.visibility_failed")?;
        let visible = observer.clone();
        native.connect_visible_notify(move |_| visible.hint());
        let minimized = observer.clone();
        native.connect_window_state_event(move |_, _| {
            minimized.hint();
            gtk::glib::Propagation::Proceed
        });
    }
    let hints = observer.clone();
    window.on_window_event(move |event| match event {
        WindowEvent::Destroyed => hints.close(),
        WindowEvent::Focused(_)
        | WindowEvent::Resized(_)
        | WindowEvent::ScaleFactorChanged { .. } => hints.hint(),
        _ => {}
    });
    observer.launch_with(window.clone(), |window, sample| {
        window
            .emit_to(
                tauri::EventTarget::webview_window(window.label()),
                EVENT,
                sample,
            )
            .is_ok()
    });
    Ok(())
}

#[tauri::command]
pub(super) async fn workspace_native_visibility(
    window: WebviewWindow,
    state: State<'_, Workspace>,
    nonce: String,
) -> Result<crate::native_visibility::NativeVisibilitySample> {
    if state.native_label().ok() != Some(window.label()) {
        return Err("workspace.denied");
    }
    session::page(&window, &state, &nonce)?;
    let snapshot = state
        .0
        .visibility
        .sample(window.clone())
        .await
        .map_err(|_| "workspace.visibility_failed")?;
    session::page(&window, &state, &nonce)?;
    if !snapshot.valid {
        return Err("workspace.visibility_failed");
    }
    Ok(snapshot)
}
