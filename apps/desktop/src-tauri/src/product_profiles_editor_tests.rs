use super::*;
use pulse_app::{local_session_root::LocalSessionRoot, product_document::ProductDocuments};
use std::{fs, os::unix::fs::PermissionsExt};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
type PageFixture = (
    SessionRegistry,
    ProductDocuments,
    pulse_app::registry::WindowToken,
    Arc<PageLaunchScope>,
);
type ProfileSource = (PathBuf, OsString, Option<PathBuf>);
fn page() -> std::result::Result<PageFixture, Box<dyn std::error::Error>> {
    let r = SessionRegistry::new();
    let d = ProductDocuments::new(r.clone());
    let w = r.open_window()?;
    d.install(w)?;
    d.started(w)?;
    let n = d.finished(w)?;
    d.confirm(w, n)?;
    let p = d.current(w, n)?;
    Ok((r, d, w, p))
}
fn fixture() -> std::io::Result<(tempfile::TempDir, ProfileSource)> {
    let t = tempfile::tempdir()?;
    let bin = t.path().join("bin");
    fs::create_dir(&bin)?;
    let sh = bin.join("sh");
    fs::write(&sh, b"#!/bin/sh\nexec /bin/sh \"$@\"\n")?;
    fs::set_permissions(&sh, fs::Permissions::from_mode(0o700))?;
    let source = (t.path().join("state"), bin.into_os_string(), None);
    Ok((t, source))
}
#[tokio::test]
async fn editor_is_single_use_cancelled_and_original_page_bound() -> TestResult {
    let (_temp, s) = fixture()?;
    let (r, d, w, p) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .editor_read(r.clone(), p.clone(), s.clone(), AutomaticShell::Sh)
        .await?;
    assert!(offer.config.args.is_empty());
    reader
        .editor_save(r.clone(), p.clone(), offer.ticket, ProfileConfig::default())
        .await?;
    assert_eq!(
        reader
            .editor_save(r.clone(), p.clone(), offer.ticket, ProfileConfig::default())
            .await,
        Err("workspace.denied")
    );
    let offer = reader
        .editor_read(r.clone(), p.clone(), s.clone(), AutomaticShell::Sh)
        .await?;
    reader.editor_cancel(&r, &p, offer.ticket)?;
    assert_eq!(
        reader
            .editor_save(r.clone(), p.clone(), offer.ticket, ProfileConfig::default())
            .await,
        Err("workspace.denied")
    );
    let offer = reader
        .editor_read(r.clone(), p.clone(), s, AutomaticShell::Sh)
        .await?;
    d.started(w)?;
    assert_eq!(
        reader
            .editor_save(r.clone(), p, offer.ticket, ProfileConfig::default())
            .await,
        Err("workspace.denied")
    );
    reader.shutdown().await?;
    r.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn editor_denies_foreign_window_expiry_and_missing_cwd_never_falls_back() -> TestResult {
    let (temp, s) = fixture()?;
    let (r, d, w, p) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let offer = reader
        .editor_read(r.clone(), p.clone(), s.clone(), AutomaticShell::Sh)
        .await?;
    let other = r.open_window()?;
    d.install(other)?;
    d.started(other)?;
    let nonce = d.finished(other)?;
    d.confirm(other, nonce)?;
    let foreign = d.current(other, nonce)?;
    assert_eq!(
        reader
            .editor_save(r.clone(), foreign, offer.ticket, ProfileConfig::default())
            .await,
        Err("workspace.denied")
    );
    reader
        .editor_ticket
        .lock()
        .map_err(|_| "lock")?
        .as_mut()
        .ok_or("ticket")?
        .deadline = Instant::now() - Duration::from_secs(1);
    assert_eq!(
        reader
            .editor_save(r.clone(), p.clone(), offer.ticket, ProfileConfig::default())
            .await,
        Err("workspace.denied")
    );
    reader.editor_cancel(&r, &p, offer.ticket)?;
    let offer = reader
        .editor_read(r.clone(), p.clone(), s.clone(), AutomaticShell::Sh)
        .await?;
    let marker = temp.path().join("must-not-run");
    reader
        .editor_save(
            r.clone(),
            p.clone(),
            offer.ticket,
            ProfileConfig {
                args: vec![
                    "-c".into(),
                    "printf invoked > \"$1\"".into(),
                    "fixture".into(),
                    marker.to_str().ok_or("path")?.into(),
                ],
                cwd: Some(temp.path().join("missing").to_str().ok_or("path")?.into()),
                ..Default::default()
            },
        )
        .await?;
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        r.clone(),
        tokio::runtime::Handle::current(),
    )));
    assert!(
        reader
            .start(
                r.clone(),
                p,
                root.clone(),
                ProfileStart {
                    preset: pulse_store::settings::TerminalPreset::Classic.into(),
                    directory: s.0,
                    path: s.1,
                    shell: s.2,
                    kind: pulse_pty::profiles::ShellKind::Sh
                }
            )
            .await
            .is_err()
    );
    assert!(!marker.exists());
    assert_eq!(root.lock().map_err(|_| "lock")?.occupied(), 0);
    reader.shutdown().await?;
    let drain = root.lock().map_err(|_| "lock")?.shutdown();
    drain.join().await?;
    d.started(w)?;
    r.shutdown()?;
    Ok(())
}
#[tokio::test]
async fn saved_literal_arguments_cwd_environment_reach_native_child_and_appearance_stays_fixed()
-> TestResult {
    let (temp, s) = fixture()?;
    let (r, _d, _w, p) = page()?;
    let reader = Arc::new(ProfileReader::new());
    let output = temp.path().join("receipt");
    let script = "printf '%s\\0%s\\0%s\\0%s\\0' \"$PWD\" \"$1\" \"$2\" \"$PROFILE_TEST\" > \"$3\"";
    let config = ProfileConfig {
        args: vec![
            "-c".into(),
            script.into(),
            "fixture".into(),
            " ; $(never executed) 中文🌍 ".into(),
            "".into(),
            output.to_str().ok_or("path")?.into(),
        ],
        cwd: Some(temp.path().to_str().ok_or("path")?.into()),
        env: vec![("PROFILE_TEST".into(), "精确 = value".into())],
        env_remove: vec!["OLD_PROFILE".into()],
        appearance: pulse_store::profile_editor::ProfileAppearance {
            font_size: Some(18),
            font_family: Some("Pulse Mono".into()),
            cursor_blink: Some(false),
            ..Default::default()
        },
    };
    let offer = reader
        .editor_read(r.clone(), p.clone(), s.clone(), AutomaticShell::Sh)
        .await?;
    reader
        .editor_save(r.clone(), p.clone(), offer.ticket, config.clone())
        .await?;
    assert!(!output.exists());
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        r.clone(),
        tokio::runtime::Handle::current(),
    )));
    let id = reader
        .start(
            r.clone(),
            p.clone(),
            root.clone(),
            ProfileStart {
                preset: pulse_store::settings::TerminalPreset::Classic.into(),
                directory: s.0.clone(),
                path: s.1.clone(),
                shell: None,
                kind: pulse_pty::profiles::ShellKind::Sh,
            },
        )
        .await?;
    let owner = root.lock().map_err(|_| "lock")?.take_for_finish(id)?;
    let receipt = tokio::time::timeout(Duration::from_secs(5), owner.finish()).await??;
    assert_eq!(receipt.native.exit_code, 0);
    let expected = format!(
        "{}\0 ; $(never executed) 中文🌍 \0\0精确 = value\0",
        temp.path().display()
    );
    assert_eq!(fs::read(output)?, expected.as_bytes());
    assert_eq!(
        root.lock()
            .map_err(|_| "lock")?
            .profile_appearance(id)
            .font_size,
        Some(18)
    );
    let offer = reader
        .editor_read(r.clone(), p.clone(), s, AutomaticShell::Sh)
        .await?;
    reader
        .editor_save(r.clone(), p, offer.ticket, ProfileConfig::default())
        .await?;
    assert_eq!(
        root.lock()
            .map_err(|_| "lock")?
            .profile_appearance(id)
            .font_size,
        Some(18)
    );
    root.lock().map_err(|_| "lock")?.collect_reaped();
    assert_eq!(root.lock().map_err(|_| "lock")?.occupied(), 0);
    reader.shutdown().await?;
    let drain = root.lock().map_err(|_| "lock")?.shutdown();
    drain.join().await?;
    r.shutdown()?;
    Ok(())
}
