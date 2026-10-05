use anyhow::{Context, Result, bail};
#[path = "settings_binding_probe.rs"]
mod binding;
use pulse_store::{
    StateStore, StoreError,
    settings::{SettingsPatch, SettingsSource},
    settings_edit::TerminalSettingEdit as Set,
    settings_file::SettingsFile,
    settings_reload::SettingsReloadCache,
    settings_scope::{ScopedSettingsEdit, SettingsScope},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
use uuid::Uuid;
const MODES: [&str; 5] = ["write", "edit-font", "edit-cursor", "read", "aba"];
fn supported() -> Result<()> {
    if !cfg!(any(target_os = "linux", target_os = "macos")) {
        bail!("settings scope probe requires supported private file platform");
    }
    Ok(())
}
fn id() -> Uuid {
    Uuid::from_u128(1)
}
fn group() -> SettingsScope {
    SettingsScope::Group(id())
}
fn host() -> SettingsScope {
    SettingsScope::Host(id())
}
fn size(value: u16) -> ScopedSettingsEdit {
    ScopedSettingsEdit::Set(Set::FontSize(value))
}
pub(super) fn helper() -> Result<()> {
    supported()?;
    let mut args = std::env::args_os().skip(2);
    let mode = args
        .next()
        .and_then(|s| s.into_string().ok())
        .context("missing mode")?;
    let path = PathBuf::from(args.next().context("missing path")?);
    if args.next().is_some() || !MODES.contains(&mode.as_str()) {
        bail!("invalid fixed helper arguments");
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let receipt=runtime.block_on(async {
        let(store,task)=StateStore::open(path.clone()).await?;
        let receipt=match mode.as_str(){
            "write"=>{if store.load_scoped_settings(group()).await?.revision()!=0||store.load_scoped_settings(host()).await?.revision()!=0{bail!("unexpected existing fixture");}store.edit_scoped_settings(group(),size(18)).await?;store.edit_scoped_settings(host(),size(22)).await?;json!({"initialized":true})},
            "edit-font"=>{store.edit_scoped_settings(group(),size(20)).await?;json!({"edited":true})},
            "edit-cursor"=>{store.edit_scoped_settings(group(),ScopedSettingsEdit::Set(Set::CursorBlink(false))).await?;json!({"edited":true})},
            "read"=>{
                let mut cache=SettingsReloadCache::new(SettingsFile::open_private(path.parent().context("missing parent")?.to_path_buf())?);cache.refresh()?;
                let layers=store.load_settings_layers(Some(id()),Some(id())).await?;let g=layers.group().context("missing group")?;let h=layers.host().context("missing host")?;
                if g.revision()!=3||g.patch().terminal.font_size!=Some(20)||g.patch().terminal.cursor_blink!=Some(false)||h.revision()!=1||h.patch().terminal.font_size!=Some(22){bail!("scoped atomic edit evidence missing");}
                let resolved=layers.resolve(Some(cache.user_patch()))?;if resolved.font_size.value!=22||resolved.font_size.source!=SettingsSource::Host||resolved.font_family.value!="Fira Code"||resolved.font_family.source!=SettingsSource::User||resolved.cursor_blink.value||resolved.cursor_blink.source!=SettingsSource::Group{bail!("layer source evidence mismatch");}
                binding::check(&store,cache.user_patch().clone()).await?;
                json!({"scopeIsolation":true,"concurrentEditsPreserved":true,"userFileChecked":true,"perFieldSourcesChecked":true,"contextSwitchChecked":true,"lateCandidateRejected":true,"contextOwnerCloseChecked":true,"registryWindowAuthorizationChecked":true,"registryRefreshChecked":true,"registryTransferChecked":true,"groupRevision":g.revision(),"hostRevision":h.revision()})
            },
            "aba"=>{let old=store.load_scoped_settings(group()).await?;store.edit_scoped_settings(group(),size(24)).await?;store.edit_scoped_settings(group(),size(20)).await?;if !matches!(store.replace_scoped_settings(old,SettingsPatch::default()).await,Err(StoreError::SettingsConflict)){bail!("old snapshot revived");}let current=store.load_scoped_settings(group()).await?;if current.revision()!=5||current.patch().terminal.cursor_blink!=Some(false)||store.load_scoped_settings(host()).await?.patch().terminal.font_size!=Some(22){bail!("ABA rejected but row not preserved");}json!({"abaRejected":true,"rowPreserved":true})},
            _=>bail!("unsupported helper mode")
        };
        task.shutdown().await?;Ok::<_,anyhow::Error>(json!({"mode":mode,"pid":std::process::id(),"workerJoined":true,"receipt":receipt}))
    })?;
    println!("{receipt}");
    Ok(())
}
async fn child(mode: &str, path: &Path) -> Result<Value> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .arg("settings-scope-helper")
        .arg(mode)
        .arg(path)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .context("settings scope child timed out")??;
    if !output.status.success() || output.stdout.len() > 4096 || output.stderr.len() > 4096 {
        bail!("settings scope child failed");
    }
    let receipt: Value = serde_json::from_slice(&output.stdout)?;
    if receipt["mode"] != mode || receipt["workerJoined"] != true {
        bail!("wrong helper receipt");
    }
    Ok(receipt)
}
fn verify(receipts: &[Value; 5]) -> Result<()> {
    let mut pids = std::collections::BTreeSet::new();
    for (receipt, mode) in receipts.iter().zip(MODES) {
        let pid = receipt["pid"]
            .as_u64()
            .filter(|p| (1..=u64::from(u32::MAX)).contains(p))
            .context("invalid process id")?;
        if receipt["mode"] != mode
            || receipt["workerJoined"].as_bool() != Some(true)
            || !pids.insert(pid)
        {
            bail!("incomplete independent process receipt");
        }
    }
    for (index, key) in [
        (0, "initialized"),
        (1, "edited"),
        (2, "edited"),
        (3, "scopeIsolation"),
        (3, "concurrentEditsPreserved"),
        (3, "userFileChecked"),
        (3, "perFieldSourcesChecked"),
        (3, "contextSwitchChecked"),
        (3, "lateCandidateRejected"),
        (3, "contextOwnerCloseChecked"),
        (3, "registryWindowAuthorizationChecked"),
        (3, "registryRefreshChecked"),
        (3, "registryTransferChecked"),
        (4, "abaRejected"),
        (4, "rowPreserved"),
    ] {
        if receipts[index]["receipt"][key].as_bool() != Some(true) {
            bail!("missing scoped setting evidence");
        }
    }
    if receipts[3]["receipt"]["groupRevision"] != 3 || receipts[3]["receipt"]["hostRevision"] != 1 {
        bail!("wrong revision evidence");
    }
    Ok(())
}
pub(super) fn run() -> Result<()> {
    supported()?;
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let user = dir.path().join("settings.toml");
    std::fs::write(
        &user,
        "# fixed isolated user fixture\nterminal.font_family='Fira Code'\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&user, std::fs::Permissions::from_mode(0o600))?;
    }
    let runtime = tokio::runtime::Runtime::new()?;
    runtime.block_on(async {
        let path = dir.path().join("state.db");
        let writer = child("write", &path).await?;
        let (a, b) = tokio::join!(child("edit-font", &path), child("edit-cursor", &path));
        let a = a?;
        let b = b?;
        let reader = child("read", &path).await?;
        let aba = child("aba", &path).await?;
        verify(&[writer, a, b, reader, aba])
    })?;
    std::fs::create_dir_all("target/probes")?;
    std::fs::write(
        "target/probes/settings-scope.json",
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":3,"independentProcesses":5,"concurrentEditors":2,"scopeIsolation":true,"concurrentEditsPreserved":true,"userFileChecked":true,"perFieldSourcesChecked":true,"abaRejected":true,"rowPreserved":true,"workerJoined":true,"contextSwitchChecked":true,"lateCandidateRejected":true,"contextOwnerCloseChecked":true,"registryWindowAuthorizationChecked":true,"registryRefreshChecked":true,"registryTransferChecked":true,"nativeWindowsCreated":false,"renderer":"rust-diagnostic","userHomeRead":false,"hostAuthorityGranted":false,"crossFileDatabaseTransaction":false}),
        )?,
    )?;
    println!(
        "PASS settings-scope: five processes, concurrent typed edits, private user file, per-field sources and ABA rejection"
    );
    Ok(())
}
#[cfg(test)]
#[path = "settings_scope_probe_tests.rs"]
mod tests;
