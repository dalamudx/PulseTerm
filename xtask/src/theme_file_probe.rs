//! Fixed private-fixture subprocess validation; not an application theme service.
use anyhow::{Context, Result, bail, ensure};
use pulse_app::theme_context::{ThemeContextError, ThemeContextOwner};
use pulse_store::{
    theme::{
        AccentChoice, NeutralScale, ThemeAppearance, ThemeDocument, ThemeImportFormat, UiThemeBase,
        import_theme,
    },
    theme_file::{ThemeFile, ThemeFileError},
    theme_io::{ThemeIoError, ThemeIoOwner},
};
use serde::{Deserialize, Serialize};
use std::{
    io::{BufRead, Read, Write},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
    time::timeout,
};
use uuid::Uuid;
const LIMIT: usize = 1024;
const DEADLINE: Duration = Duration::from_secs(5);
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Phase {
    Loaded,
    Committed,
    Conflict,
    Read,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u8,
    pid: u32,
    phase: Phase,
    slot_checked: bool,
    roundtrip_checked: bool,
    background_io_checked: bool,
    context_checked: bool,
}
fn parse_receipt(line: &str, pid: u32, phase: Phase) -> Result<()> {
    ensure!(
        line.len() <= LIMIT && line.ends_with('\n'),
        "theme.file.probe.invalid_receipt"
    );
    let r: Receipt = serde_json::from_str(line)
        .map_err(|_| anyhow::anyhow!("theme.file.probe.invalid_receipt"))?;
    ensure!(
        r.schema == 3
            && r.pid == pid
            && pid != 0
            && r.phase == phase
            && r.slot_checked
            && r.roundtrip_checked
            && r.background_io_checked
            && r.context_checked,
        "theme.file.probe.invalid_receipt"
    );
    Ok(())
}
fn id() -> Uuid {
    Uuid::from_u128(1)
}
fn document(name: &str) -> Result<ThemeDocument> {
    ensure!(
        ["initial", "committed", "stale"].contains(&name),
        "invalid fixed document"
    );
    let mut colors = serde_json::Map::new();
    for field in [
        "black",
        "red",
        "green",
        "yellow",
        "blue",
        "purple",
        "cyan",
        "white",
        "brightBlack",
        "brightRed",
        "brightGreen",
        "brightYellow",
        "brightBlue",
        "brightPurple",
        "brightCyan",
        "brightWhite",
    ] {
        colors.insert(field.into(), serde_json::Value::String("#112233".into()));
    }
    for (field, value) in [
        ("foreground", "#ffffff"),
        ("background", "#000000"),
        ("cursorColor", "#ffffff"),
    ] {
        colors.insert(field.into(), serde_json::Value::String(value.into()));
    }
    colors.insert(
        "name".into(),
        serde_json::Value::String(format!("Fixture {name}")),
    );
    let scheme = import_theme(
        ThemeImportFormat::WindowsTerminal,
        &serde_json::to_string(&colors)?,
        None,
    )?;
    Ok(ThemeDocument::from_import(
        &scheme,
        ThemeAppearance::Dark,
        UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?),
    )?)
}
fn supported() -> Result<()> {
    ensure!(
        cfg!(any(target_os = "linux", target_os = "macos")),
        "theme file probe requires supported private-file platform"
    );
    Ok(())
}
fn emit(phase: Phase) -> Result<()> {
    println!(
        "{}",
        serde_json::to_string(&Receipt {
            schema: 3,
            pid: std::process::id(),
            phase,
            slot_checked: true,
            roundtrip_checked: true,
            background_io_checked: true,
            context_checked: true
        })?
    );
    std::io::stdout().flush()?;
    Ok(())
}
async fn child(mode: &str, path: PathBuf) -> Result<()> {
    supported()?;
    ensure!(
        ["writer", "stale", "read", "waiting"].contains(&mode),
        "invalid fixed child mode"
    );
    let file = tokio::task::spawn_blocking(move || ThemeFile::open_private(path, id())).await??;
    ensure!(file.id() == id(), "slot identity mismatch");
    let owner = ThemeIoOwner::new(file);
    let handle = owner.handle();
    let captured = handle.begin_load()?.ready().await?;
    let initial = if mode == "read" {
        "committed"
    } else {
        "initial"
    };
    ensure!(
        captured.document()? == Some(document(initial)?),
        "captured document mismatch"
    );
    let context = ThemeContextOwner::new(handle.clone())?;
    let themes = context.handle();
    let candidate = themes.begin_load()?.ready().await?;
    ensure!(themes.last_good()?.is_none(), "read silently published");
    context.invalidate()?;
    ensure!(
        matches!(themes.confirm(candidate), Err(ThemeContextError::Stale)),
        "old epoch confirmed"
    );
    let published = themes.confirm(themes.begin_load()?.ready().await?)?;
    ensure!(
        themes.is_current(&published) && published.document() == &document(initial)?,
        "context document mismatch"
    );
    if mode == "read" {
        context.close();
        ensure!(
            !themes.is_current(&published) && handle.is_open(),
            "context close revoked shared IO"
        );
        return emit(Phase::Read);
    }
    emit(Phase::Loaded)?;
    let go = tokio::task::spawn_blocking(|| {
        let mut go = String::new();
        std::io::stdin().lock().take(4).read_line(&mut go)?;
        Ok::<_, std::io::Error>(go)
    })
    .await??;
    ensure!(go == "GO\n" && mode != "waiting", "invalid release");
    let result = handle
        .begin_save(
            captured,
            document(if mode == "writer" {
                "committed"
            } else {
                "stale"
            })?,
        )?
        .ready()
        .await;
    match (mode, result) {
        ("writer", Ok(saved)) => {
            ensure!(
                saved.document()? == Some(document("committed")?),
                "commit image mismatch"
            );
            context.invalidate()?;
            let next = themes.confirm(themes.begin_load()?.ready().await?)?;
            ensure!(
                !themes.is_current(&published)
                    && themes.is_current(&next)
                    && next.document() == &document("committed")?,
                "saved context not recaptured"
            );
            emit(Phase::Committed)
        }
        ("stale", Err(ThemeIoError::File(ThemeFileError::Conflict))) => {
            ensure!(
                handle.begin_load()?.ready().await?.document()? == Some(document("committed")?),
                "conflict overwrote data"
            );
            context.invalidate()?;
            let next = themes.confirm(themes.begin_load()?.ready().await?)?;
            ensure!(
                !themes.is_current(&published)
                    && themes.is_current(&next)
                    && next.document() == &document("committed")?,
                "conflict context not recaptured"
            );
            emit(Phase::Conflict)
        }
        _ => bail!("unexpected fixed save outcome"),
    }
}
pub(super) fn helper() -> Result<()> {
    supported()?;
    let mut args = std::env::args_os().skip(2);
    let mode = args
        .next()
        .and_then(|s| s.into_string().ok())
        .context("missing fixed mode")?;
    let path = PathBuf::from(args.next().context("missing fixture directory")?);
    ensure!(args.next().is_none(), "unexpected fixture argument");
    tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?
        .block_on(child(&mode, path))
}
struct Owned {
    child: Child,
    out: BufReader<ChildStdout>,
    input: Option<ChildStdin>,
    pid: u32,
}
impl Owned {
    fn spawn(path: &Path, mode: &str) -> Result<Self> {
        let mut child = Command::new(std::env::current_exe()?)
            .arg("probe-theme-file-child")
            .arg(mode)
            .arg(path)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::inherit())
            .kill_on_drop(true)
            .spawn()?;
        let pid = child.id().context("missing child PID")?;
        let out = BufReader::new(child.stdout.take().context("missing receipt pipe")?);
        let input = child.stdin.take();
        Ok(Self {
            child,
            out,
            input,
            pid,
        })
    }
    async fn expect(&mut self, phase: Phase) -> Result<()> {
        let mut line = String::new();
        timeout(
            DEADLINE,
            (&mut self.out)
                .take((LIMIT + 1) as u64)
                .read_line(&mut line),
        )
        .await
        .context("receipt deadline")??;
        parse_receipt(&line, self.pid, phase)
    }
    async fn release(&mut self) -> Result<()> {
        timeout(
            DEADLINE,
            self.input
                .as_mut()
                .context("missing release pipe")?
                .write_all(b"GO\n"),
        )
        .await
        .context("release deadline")??;
        Ok(())
    }
    async fn finish(&mut self) -> Result<()> {
        let status = timeout(DEADLINE, self.child.wait())
            .await
            .context("exit deadline")??;
        ensure!(status.success(), "fixed child exit failed");
        let mut trailing = String::new();
        timeout(
            DEADLINE,
            (&mut self.out)
                .take((LIMIT + 1) as u64)
                .read_to_string(&mut trailing),
        )
        .await
        .context("receipt EOF deadline")??;
        ensure!(trailing.is_empty(), "trailing child output");
        Ok(())
    }
    async fn retire(&mut self) -> Result<()> {
        if self.child.try_wait()?.is_none() {
            timeout(DEADLINE, self.child.kill())
                .await
                .context("child retirement deadline")??;
        }
        Ok(())
    }
}
async fn sequence(path: &Path, children: &mut Vec<Owned>) -> Result<()> {
    children.push(Owned::spawn(path, "writer")?);
    children[0].expect(Phase::Loaded).await?;
    children.push(Owned::spawn(path, "stale")?);
    children[1].expect(Phase::Loaded).await?;
    ensure!(children[0].pid != children[1].pid, "child PID collision");
    children.push(Owned::spawn(path, "waiting")?);
    children[2].expect(Phase::Loaded).await?;
    ensure!(
        children.iter().take(2).all(|c| c.pid != children[2].pid),
        "waiting PID collision"
    );
    children[2].retire().await?;
    ensure!(
        children[2].finish().await.is_err(),
        "killed child falsely accepted"
    );
    let verify_path = path.to_path_buf();
    tokio::task::spawn_blocking(move || {
        ensure!(
            ThemeFile::open_private(verify_path, id())?
                .load()?
                .document()?
                == Some(document("initial")?),
            "cancelled handoff modified theme"
        );
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    children[0].release().await?;
    children[0].expect(Phase::Committed).await?;
    children[0].finish().await?;
    children[1].release().await?;
    children[1].expect(Phase::Conflict).await?;
    children[1].finish().await?;
    children.push(Owned::spawn(path, "read")?);
    ensure!(
        children.iter().take(3).all(|c| c.pid != children[3].pid),
        "reader PID collision"
    );
    children[3].expect(Phase::Read).await?;
    children[3].finish().await
}
pub(super) fn run() -> Result<()> {
    supported()?;
    #[cfg(unix)]
    let d = {
        use std::os::unix::fs::PermissionsExt;
        let d = tempfile::tempdir()?;
        std::fs::set_permissions(d.path(), std::fs::Permissions::from_mode(0o700))?;
        d
    };
    #[cfg(not(unix))]
    let d = tempfile::tempdir()?;
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let path = d.path().to_path_buf();
        tokio::task::spawn_blocking(move || {
            let f = ThemeFile::open_private(path, id())?;
            f.save(&f.load()?, &document("initial")?)?;
            Ok::<_, anyhow::Error>(())
        })
        .await??;
        let mut children = Vec::new();
        let result = sequence(d.path(), &mut children).await;
        let mut cleanup = Ok(());
        for c in &mut children {
            if let Err(e) = c.retire().await
                && cleanup.is_ok()
            {
                cleanup = Err(e);
            }
        }
        result?;
        cleanup?;
        Ok::<_, anyhow::Error>(())
    })?;
    println!(
        "PASS theme-file: four independent PID-bound background IO and epoch-confirmation processes, cancelled unreleased child reaped, reopened complete theme, stale snapshot refused and joined children"
    );
    Ok(())
}
#[cfg(test)]
#[path = "theme_file_probe_tests.rs"]
mod tests;
