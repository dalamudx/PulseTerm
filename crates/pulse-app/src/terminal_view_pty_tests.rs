#![allow(clippy::unwrap_used)]
//! Actual private Profile DB → root PTY → current-page v2 view/input/EOF. Not WebView.
use super::*;
use crate::{
    local_launch::StoredProfileLaunch, local_session_root::LocalSessionRoot,
    product_document::ProductDocuments,
};
use pulse_core::TermSize;
use pulse_pty::{
    launch::LocalSessionEnvironment,
    profiles::{ShellCandidate, ShellKind},
};
use std::{os::unix::fs::PermissionsExt, sync::Mutex};
use tokio::time::{Duration, timeout};
type TestResult = Result<(), Box<dyn std::error::Error>>;
async fn next(
    view: &mut AttachedTerminalView,
) -> Result<(pulse_ipc::attach::Part, Vec<u8>), Box<dyn std::error::Error>> {
    let wire = timeout(Duration::from_secs(5), view.transport.wire.recv())
        .await?
        .ok_or("wire closed before expected data/EOF")?;
    let frame = pulse_ipc::decode(&wire)?;
    let fragment = pulse_ipc::attach_v2::decode_fragment(frame.payload)?;
    assert_eq!(fragment.generation, view.binding.generation());
    let part = fragment.part;
    let body = fragment.body.to_vec();
    view.binding.acknowledge(frame.sequence)?;
    Ok((part, body))
}
#[tokio::test]
async fn private_stored_profile_pty_echo_survives_view_drop_refresh_and_ordered_eof() -> TestResult
{
    let dir = crate::test_tempdir()?;
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
    let program = dir.path().join("sh");
    std::fs::write(
        &program,
        "#!/bin/sh\nexec /bin/sh -c 'printf \"ready\\n\"; while IFS= read -r line; do printf \"echo:%s\\n\" \"$line\"; if [ \"$line\" = quit ]; then exit 7; fi; done'\n",
    )?;
    std::fs::set_permissions(&program, std::fs::Permissions::from_mode(0o700))?;
    let candidates = [ShellCandidate::new(ShellKind::Sh, program)?];
    let (store, store_task) = pulse_store::StateStore::open(dir.path().join("state.db")).await?;
    let r = SessionRegistry::new();
    let owner = r.open_window()?;
    let docs = ProductDocuments::new(r.clone());
    docs.install(owner)?;
    docs.started(owner)?;
    let nonce = docs.finished(owner)?;
    docs.confirm(owner, nonce)?;
    let page = docs.current(owner, nonce)?;
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        r.clone(),
        tokio::runtime::Handle::current(),
    )));
    let context = LocalSessionEnvironment {
        application_version: "test-terminal-view".into(),
        pane: None,
    };
    let id = timeout(
        Duration::from_secs(5),
        crate::local_session_root::start_stored_page_local(
            &r,
            &page,
            &store,
            &root,
            StoredProfileLaunch {
                cwd: None,
                candidates: &candidates,
                kind: ShellKind::Sh,
                context: &context,
                size: TermSize::new(32, 8)?,
                history: 16,
            },
        ),
    )
    .await??;
    let mut initial =
        TerminalViewBinding::attach_v2(&r, page.clone(), id, AttachmentConfig::default()).await?;
    super::tests::bootstrap(&mut initial).await?;
    initial.binding.detach()?;
    let _ = timeout(Duration::from_secs(5), initial.transport.completion).await?;
    assert_eq!(
        r.state(owner, id)?.lifecycle,
        pulse_term::session::Lifecycle::Running
    );
    let mut old = TerminalViewBinding::attach_v2(&r, page, id, AttachmentConfig::default()).await?;
    super::tests::bootstrap(&mut old).await?;
    old.binding.input(b"hello\n".to_vec()).await?;
    let mut observed = Vec::new();
    loop {
        let (part, bytes) = next(&mut old).await?;
        if part == pulse_ipc::attach::Part::Data {
            observed.extend(bytes);
        }
        assert_ne!(part, pulse_ipc::attach::Part::End);
        if observed
            .windows(b"echo:hello".len())
            .any(|w| w == b"echo:hello")
        {
            break;
        }
    }
    let before = r.state(owner, id)?.parsed_bytes;
    docs.started(owner)?;
    let fresh_nonce = docs.finished(owner)?;
    docs.confirm(owner, fresh_nonce)?;
    let current = docs.current(owner, fresh_nonce)?;
    let mut fresh =
        TerminalViewBinding::attach_v2(&r, current.clone(), id, AttachmentConfig::default())
            .await?;
    super::tests::bootstrap(&mut fresh).await?;
    assert!(fresh.palette.boundary.parsed_bytes >= before);
    assert!(old.binding.input(b"wrong\n".to_vec()).await.is_err());
    assert!(old.binding.captured_palette().is_err());
    assert!(old.binding.acknowledge(1).is_err());
    drop(old.binding);
    let _ = timeout(Duration::from_secs(5), old.transport.completion).await?;
    fresh.binding.check_current()?;
    let new_size = TermSize::new(48, 10)?;
    fresh.binding.resize(new_size).await?;
    assert_eq!(r.state(owner, id)?.size, new_size);
    assert!(fresh.binding.input(b"old-size\n".to_vec()).await.is_err());
    let mut resized =
        TerminalViewBinding::attach_v2(&r, current, id, AttachmentConfig::default()).await?;
    super::tests::bootstrap(&mut resized).await?;
    assert_eq!(resized.palette.boundary.size, new_size);
    assert_ne!(fresh.binding.generation(), resized.binding.generation());
    drop(fresh.binding);
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await?;
    let mut fresh = resized;
    // A late admission error cannot prove that bytes were not written. Send only once.
    let _admission = fresh.binding.input(b"quit\n".to_vec()).await;
    let mut final_output = Vec::new();
    loop {
        let (part, bytes) = next(&mut fresh).await?;
        if part == pulse_ipc::attach::Part::Data {
            final_output.extend(bytes);
        }
        if part == pulse_ipc::attach::Part::End {
            break;
        }
    }
    assert!(
        final_output
            .windows(b"echo:quit".len())
            .any(|w| w == b"echo:quit")
    );
    assert!(!final_output.windows(b"wrong".len()).any(|w| w == b"wrong"));
    let _ = timeout(Duration::from_secs(5), fresh.transport.completion).await??;
    drop(fresh.binding);
    let cleanup = root.lock().unwrap().take_for_finish(id)?.finish();
    let receipt = timeout(Duration::from_secs(5), cleanup).await??;
    assert_eq!(receipt.native.exit_code, 7);
    root.lock().unwrap().collect_reaped();
    assert_eq!(root.lock().unwrap().occupied(), 0);
    docs.remove(owner)?;
    let drain = root.lock().unwrap().shutdown();
    timeout(Duration::from_secs(5), drain.join()).await??;
    r.shutdown()?;
    store_task.shutdown().await?;
    Ok(())
}
