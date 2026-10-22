//! Generate Tauri configuration only when the native desktop feature is enabled.
fn main() {
    #[cfg(feature = "desktop")]
    {
        // generate_context! embeds assets in every feature combination, including debug builds.
        // tauri-build may otherwise track only its canonical config or production asset path.
        println!("cargo:rerun-if-changed=../../../ui/dist");
        println!("cargo:rerun-if-changed=workspace/tauri.conf.json");
        tauri_build::build();
    }
}
