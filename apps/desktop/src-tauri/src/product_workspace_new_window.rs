//! Native empty-window creation; callers must consume an original-page confirmation first.
use super::*;

pub(super) fn create(app: &tauri::AppHandle, origin: &Workspace) -> Result<String> {
    let application = origin.0.application.clone();
    if !application.exit.accepting_windows() || !application.window_drains.accepting() {
        return Err("workspace.denied");
    }
    let registration = Arc::new(
        application
            .directory
            .reserve(false)
            .map_err(|_| "workspace.denied")?,
    );
    let native = Arc::new(app::NativeBinding::new(
        application.directory.clone(),
        registration,
    )?);
    let label = native.label().as_str().to_owned();
    let profiles = Arc::new(profiles::ProfileReader::with_storage(
        application.profile_storage.clone(),
    ));
    let state = match Workspace::from_native(application, native.clone(), profiles) {
        Ok(state) => state,
        Err(error) => {
            let _ = native.creation_failed();
            return Err(error);
        }
    };
    if app
        .state::<window_state::Contexts<Workspace>>()
        .install(&label, state.clone())
        .is_err()
    {
        let _ = native.creation_failed();
        let _ = state.retire_window();
        return Err("workspace.window_unconfirmed");
    }
    let loaded = state.clone();
    let builder = WebviewWindowBuilder::new(app, &label, WebviewUrl::App("workspace.html".into()))
        .title("PulseTerm · 工作区")
        .inner_size(960.0, 640.0)
        .devtools(false)
        .focused(true)
        .on_page_load(move |window, payload| {
            if loaded
                .on_page_load(&window, payload.event(), payload.url())
                .is_err()
            {
                let _ = window.close();
            }
        });
    let window = match builder.build() {
        Ok(window) => window,
        Err(_) => {
            let _ = app
                .state::<window_state::Contexts<Workspace>>()
                .retire(&label);
            let _ = native.creation_failed();
            let _ = state.retire_window();
            return Err("workspace.window_unconfirmed");
        }
    };
    let closing = state.clone();
    let closing_app = app.clone();
    let closing_label = label.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Destroyed) {
            let _ = closing_app
                .state::<window_state::Contexts<Workspace>>()
                .retire(&closing_label);
            let _ = closing
                .0
                .native
                .as_ref()
                .ok_or("workspace.denied")
                .and_then(|binding| binding.destroyed());
            let _ = closing.retire_window();
        }
    });
    if visibility::install(&window, &state).is_err() {
        let _ = state.retire_window();
        let _ = window.close();
        return Err("workspace.visibility_failed");
    }
    if native.publish().is_err() {
        let _ = state.retire_window();
        let _ = window.close();
        return Err("workspace.window_unconfirmed");
    }
    eprintln!("workspace.secondary_window_created");
    Ok(label)
}
