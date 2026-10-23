//! Separate opt-in product workspace, never the validation command runner.
fn main() {
    pulseterm_desktop::run_workspace(tauri::generate_context!("workspace/tauri.conf.json"));
}
