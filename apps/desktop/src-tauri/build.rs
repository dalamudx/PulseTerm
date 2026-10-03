//! Generate Tauri configuration only when the native desktop feature is enabled.
fn main() {
    #[cfg(feature = "desktop")]
    tauri_build::build();
}
