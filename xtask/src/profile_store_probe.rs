use anyhow::{Context, Result, bail};
#[cfg(test)]
#[path = "profile_store_tests.rs"]
mod tests;
use pulse_app::profile_state::{catalog_preferences, restore_catalog};
use pulse_pty::profiles::{ShellCandidate, ShellKind};
use pulse_store::{
    StateStore,
    profiles::{AutoProfileEdit, AutoProfilePreferences, AutomaticShell},
};
use serde_json::{Value, json};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};
pub(super) fn helper() -> Result<()> {
    let mut args = std::env::args_os().skip(2);
    let mode = args.next().context("missing mode")?;
    let path = PathBuf::from(args.next().context("missing database")?);
    if args.next().is_some() {
        bail!("extra helper argument");
    }
    let mode = mode.to_str().context("invalid mode")?;
    if !matches!(
        mode,
        "write" | "read" | "edit-fish" | "edit-sh" | "conflict"
    ) {
        bail!("invalid mode");
    }
    let r = tokio::runtime::Runtime::new()?;
    let receipt = r.block_on(async {
        let (store, task) = StateStore::open(path).await?;
        let mut expected = AutoProfilePreferences::default();
        expected.set_hidden(AutomaticShell::Bash, true)?;
        expected.suppress(AutomaticShell::Zsh);
        let receipt = if mode == "write" {
            store
                .save_auto_profiles(AutoProfilePreferences::default(), expected)
                .await?;
            json!({"written":true})
        } else if mode == "edit-fish" || mode == "edit-sh" {
            let shell = if mode == "edit-fish" { AutomaticShell::Fish } else { AutomaticShell::Sh };
            let committed = store.edit_auto_profiles(AutoProfileEdit::Visibility { shell, hidden:true }).await?;
            if !committed.hidden().any(|s|s==shell) { bail!("committed edit missing"); }
            json!({"edited":true})
        } else if mode == "conflict" {
            let before=store.load_auto_profiles().await?;
            let mut replacement=AutoProfilePreferences::default();replacement.set_hidden(AutomaticShell::Pwsh,true)?;
            if store.save_auto_profiles(AutoProfilePreferences::default(), replacement).await != Err(pulse_store::StoreError::Conflict) || store.load_auto_profiles().await? != before { bail!("stale snapshot was not rejected atomically"); }
            json!({"staleSnapshotRejected":true,"rowPreserved":true})
        } else {
            let p = store.load_auto_profiles().await?;
            expected.set_hidden(AutomaticShell::Fish, true)?;
            expected.set_hidden(AutomaticShell::Sh, true)?;
            if p != expected {
                bail!("preferences changed across process restart");
            }
            let mut c = restore_catalog(&p, &[])?;
            if catalog_preferences(&c)? != p {
                bail!("missing discovery lost preferences");
            }
            // Metadata fixture only: candidate paths never execute in this probe.
            let executable = std::env::current_exe()?;
            c.reconcile(&[
                ShellCandidate::new(ShellKind::Bash, executable.clone())?,
                ShellCandidate::new(ShellKind::Zsh, executable)?,
            ])?;
            if !c
                .profile(ShellKind::Bash)
                .context("missing Bash")?
                .is_hidden()
                || c.profile(ShellKind::Zsh).is_some()
                || catalog_preferences(&c)? != p
            {
                bail!("restored catalog does not preserve preferences");
            }
            json!({"hiddenRestored":true,"deletedSuppressed":true,"delayedDiscoveryRetained":true,"atomicEditsPreserved":true})
        };
        drop(store);
        task.join().await?;
        Ok::<_, anyhow::Error>(
            json!({"mode":mode,"pid":std::process::id(),"workerJoined":true,"receipt":receipt}),
        )
    })?;
    println!("{receipt}");
    Ok(())
}
async fn child(mode: &str, path: &Path) -> Result<Value> {
    let mut command = tokio::process::Command::new(std::env::current_exe()?);
    command
        .arg("profile-store-helper")
        .arg(mode)
        .arg(path)
        .kill_on_drop(true);
    let output = tokio::time::timeout(Duration::from_secs(5), command.output())
        .await
        .context("profile store helper timed out")??;
    if !output.status.success() || output.stdout.len() > 2048 || output.stderr.len() > 2048 {
        bail!("profile store helper failed");
    }
    let v: Value = serde_json::from_slice(&output.stdout)?;
    if v["mode"] != mode || v["workerJoined"] != true || v["pid"].as_u64().is_none() {
        bail!("invalid helper receipt");
    }
    Ok(v)
}
fn verify(
    writer: &Value,
    fish: &Value,
    sh: &Value,
    conflict: &Value,
    reader: &Value,
) -> Result<()> {
    let mut pids = std::collections::BTreeSet::new();
    for (v, mode) in [
        (writer, "write"),
        (fish, "edit-fish"),
        (sh, "edit-sh"),
        (conflict, "conflict"),
        (reader, "read"),
    ] {
        let pid = v["pid"]
            .as_u64()
            .filter(|p| (1..=u64::from(u32::MAX)).contains(p))
            .context("invalid process id")?;
        if v["mode"] != mode || v["workerJoined"] != true || !pids.insert(pid) {
            bail!("invalid independent process evidence");
        }
    }
    if writer["receipt"]["written"] != true
        || fish["receipt"]["edited"] != true
        || sh["receipt"]["edited"] != true
        || conflict["receipt"]["staleSnapshotRejected"] != true
        || conflict["receipt"]["rowPreserved"] != true
        || reader["receipt"]["hiddenRestored"] != true
        || reader["receipt"]["deletedSuppressed"] != true
        || reader["receipt"]["delayedDiscoveryRetained"] != true
        || reader["receipt"]["atomicEditsPreserved"] != true
    {
        bail!("incomplete store edit evidence");
    }
    Ok(())
}

pub(super) fn run() -> Result<()> {
    let dir = tempfile::tempdir()?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    }
    let runtime = tokio::runtime::Runtime::new()?;
    let receipt = runtime.block_on(async {
        let path=dir.path().join("pulse.db");
        let writer=child("write",&path).await?;
        let (fish,sh)=tokio::join!(child("edit-fish",&path),child("edit-sh",&path));
        let fish=fish?;let sh=sh?;
        let conflict=child("conflict",&path).await?;
        let reader=child("read",&path).await?;
        verify(&writer,&fish,&sh,&conflict,&reader)?;
        Ok::<_,anyhow::Error>(json!({"schemaVersion":2,"independentProcesses":5,"concurrentEditors":2,"atomicEditsPreserved":true,"staleSnapshotRejected":true,"hiddenRestored":true,"deletedSuppressed":true,"delayedDiscoveryRetained":true,"workerJoined":true,"candidateExecuted":false}))
    })?;
    std::fs::create_dir_all("target/probes")?;
    std::fs::write(
        "target/probes/profile-store.json",
        serde_json::to_vec_pretty(&receipt)?,
    )?;
    println!(
        "PASS profile-store: five independent processes, concurrent typed edits, stale snapshot refused, joined workers"
    );
    Ok(())
}
