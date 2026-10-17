use super::*;
use crate::theme::{
    AccentChoice, NeutralScale, ThemeAppearance, ThemeImportFormat, UiThemeBase, import_theme,
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Mutex, mpsc},
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;
static TESTS: Mutex<()> = Mutex::new(());
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn rt() -> std::io::Result<Runtime> {
    Builder::new_current_thread()
        .enable_time()
        .max_blocking_threads(1)
        .build()
}
fn fixture() -> Result<(tempfile::TempDir, ThemeIoOwner), Box<dyn std::error::Error>> {
    let d = tempfile::tempdir()?;
    fs::set_permissions(d.path(), fs::Permissions::from_mode(0o700))?;
    let f = ThemeFile::open_private(d.path().into(), Uuid::from_u128(1))?;
    Ok((d, ThemeIoOwner::new(f)))
}
fn doc() -> Result<ThemeDocument, ThemeIoError> {
    let names = [
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
        "foreground",
        "background",
    ];
    let mut p = serde_json::Map::new();
    for key in names {
        p.insert(key.into(), serde_json::Value::String("#112233".into()));
    }
    p.insert("name".into(), serde_json::Value::String("fixture".into()));
    let text = serde_json::to_string(&p).map_err(|_| ThemeIoError::Worker)?;
    let i = import_theme(ThemeImportFormat::WindowsTerminal, &text, None)
        .map_err(ThemeFileError::Document)?;
    Ok(ThemeDocument::from_import(
        &i,
        ThemeAppearance::Dark,
        UiThemeBase::new(
            NeutralScale::Slate,
            AccentChoice::parse("cyan").map_err(ThemeFileError::Document)?,
        ),
    )
    .map_err(ThemeFileError::Document)?)
}
fn observe<T>(rt: &Runtime, rx: oneshot::Receiver<T>) -> Result<T, Box<dyn std::error::Error>> {
    Ok(rt.block_on(async { tokio::time::timeout(Duration::from_secs(5), rx).await })??)
}
#[test]
fn timed_out_ready_discards_delivery_but_keeps_quota_until_actual_exit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let (start, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = {
        let _enter = rt.enter();
        dispatch(&owner.handle(), move |s| {
            let _sent = start.send(());
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(|_| ThemeIoError::Worker)?;
            Ok(s.file.load()?)
        })?
    };
    observe(&rt, rx)?;
    assert!(
        rt.block_on(async {
            tokio::time::timeout(Duration::from_millis(1), request.ready()).await
        })
        .is_err()
    );
    assert!(ACTIVE.load(Ordering::Acquire));
    {
        let _enter = rt.enter();
        assert!(matches!(
            owner.handle().begin_load(),
            Err(ThemeIoError::Busy)
        ));
    }
    release.send(())?;
    // Single blocking lane: its next task cannot complete before the detached closure has exited.
    rt.block_on(rt.spawn_blocking(|| ()))?;
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn owner_drop_skips_queued_io_and_discards_its_late_result() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let h = owner.handle();
    let (start, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let occupied = rt.spawn_blocking(move || {
        let _sent = start.send(());
        wait.recv_timeout(Duration::from_secs(5))
    });
    observe(&rt, rx)?;
    let seen = Arc::new(AtomicBool::new(false));
    let executed = seen.clone();
    let request = {
        let _enter = rt.enter();
        dispatch(&h, move |s| {
            executed.store(true, Ordering::Release);
            Ok(s.file.load()?)
        })?
    };
    drop(owner);
    assert!(ACTIVE.load(Ordering::Acquire));
    assert!(matches!(h.begin_load(), Err(ThemeIoError::Closed)));
    release.send(())?;
    rt.block_on(occupied)??;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(ThemeIoError::Closed)
    ));
    assert!(!seen.load(Ordering::Acquire));
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn real_load_save_reopen_and_exact_conflict() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let h = owner.handle();
    rt.block_on(async {
        let original = h.begin_load()?.ready().await?;
        assert!(!original.exists());
        let saved = h.begin_save(original.clone(), doc()?)?.ready().await?;
        assert!(saved.document()? == Some(doc()?));
        assert!(matches!(
            h.begin_save(original, doc()?)?.ready().await,
            Err(ThemeIoError::File(ThemeFileError::Conflict))
        ));
        let reloaded = h.begin_load()?.ready().await?;
        assert!(reloaded == saved);
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    Ok(())
}
#[test]
fn no_runtime_and_closed_weak_handles_do_not_take_quota() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let (_d, owner) = fixture()?;
    let h = owner.handle();
    assert!(matches!(h.begin_load(), Err(ThemeIoError::Runtime)));
    assert!(!ACTIVE.load(Ordering::Acquire));
    owner.close();
    assert!(matches!(h.begin_load(), Err(ThemeIoError::Closed)));
    drop(owner);
    assert!(matches!(h.begin_load(), Err(ThemeIoError::Closed)));
    Ok(())
}
#[test]
fn runtime_shutdown_retains_active_io_guard_and_quota_until_real_exit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let runtime = rt()?;
    let (_directory, owner) = fixture()?;
    let h = owner.handle();
    struct Lifetime(Option<oneshot::Sender<()>>);
    impl Drop for Lifetime {
        fn drop(&mut self) {
            if let Some(sender) = self.0.take() {
                let _sent = sender.send(());
            }
        }
    }
    let (notice, mut exited) = oneshot::channel();
    let (started, entered) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = {
        let _enter = runtime.enter();
        dispatch_retaining(&h, Lifetime(Some(notice)), move |s| {
            let _sent = started.send(());
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(|_| ThemeIoError::Worker)?;
            Ok(s.file.load()?)
        })?
    };
    observe(&runtime, entered)?;
    let supervisor = runtime.spawn(async move { request.ready().await });
    runtime.shutdown_timeout(Duration::ZERO);
    let replacement = rt()?;
    assert!(replacement.block_on(supervisor).is_err());
    assert!(matches!(
        exited.try_recv(),
        Err(oneshot::error::TryRecvError::Empty)
    ));
    {
        let _enter = replacement.enter();
        assert!(matches!(h.begin_load(), Err(ThemeIoError::Busy)));
    }
    release.send(())?;
    observe(&replacement, exited)?;
    assert!(!ACTIVE.load(Ordering::Acquire));
    replacement.block_on(async { h.begin_load()?.ready().await })?;
    Ok(())
}
#[test]
fn active_cancel_keeps_global_quota_until_worker_exit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let h = owner.handle();
    let (started, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = {
        let _enter = rt.enter();
        dispatch(&h, move |s| {
            let _sent = started.send(());
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(|_| ThemeIoError::Worker)?;
            Ok(s.file.load()?)
        })?
    };
    observe(&rt, rx)?;
    let cancelled = request.cancel();
    {
        let _enter = rt.enter();
        assert!(matches!(h.begin_load(), Err(ThemeIoError::Busy)));
        let (_other_directory, other_owner) = fixture()?;
        assert!(matches!(
            other_owner.handle().begin_load(),
            Err(ThemeIoError::Busy)
        ));
    }
    assert!(ACTIVE.load(Ordering::Acquire));
    release.send(())?;
    rt.block_on(cancelled.join())?;
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn queued_cancel_skips_io_but_does_not_release_admission_early() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let (start, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let occupied = rt.spawn_blocking(move || {
        let _sent = start.send(());
        wait.recv_timeout(Duration::from_secs(5))
    });
    observe(&rt, rx)?;
    let seen = Arc::new(AtomicBool::new(false));
    let executed = seen.clone();
    let request = {
        let _enter = rt.enter();
        dispatch(&owner.handle(), move |s| {
            executed.store(true, Ordering::Release);
            Ok(s.file.load()?)
        })?
    };
    let cancelled = request.cancel();
    assert!(ACTIVE.load(Ordering::Acquire));
    release.send(())?;
    rt.block_on(occupied)??;
    rt.block_on(cancelled.join())?;
    assert!(!seen.load(Ordering::Acquire));
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn close_after_commit_discards_receipt_without_rollback() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (d, owner) = fixture()?;
    let (started, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = {
        let _enter = rt.enter();
        dispatch(&owner.handle(), move |s| {
            let saved = s.file.save(&s.file.load()?, &doc()?)?;
            let _sent = started.send(());
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(|_| ThemeIoError::Worker)?;
            Ok(saved)
        })?
    };
    observe(&rt, rx)?;
    owner.close();
    assert!(ACTIVE.load(Ordering::Acquire));
    release.send(())?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(ThemeIoError::Closed)
    ));
    let f = ThemeFile::open_private(d.path().into(), Uuid::from_u128(1))?;
    assert!(f.load()?.document()? == Some(doc()?));
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn closure_observation_is_weak_late_and_one_way() -> TestResult {
    use std::future::Future;
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let (_d, owner) = fixture()?;
    let h = owner.handle();
    let mut future = Box::pin(h.observe_close().wait());
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.as_mut().poll(&mut cx).is_pending());
    owner.close();
    owner.close();
    assert!(future.as_mut().poll(&mut cx).is_ready());
    drop(owner);
    assert!(h.shared.upgrade().is_none());
    assert!(
        Box::pin(h.observe_close().wait())
            .as_mut()
            .poll(&mut cx)
            .is_ready()
    );
    Ok(())
}
#[test]
fn closure_notification_never_releases_a_running_operation_quota() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let (start, rx) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = {
        let _enter = rt.enter();
        dispatch(&owner.handle(), move |s| {
            let _sent = start.send(());
            wait.recv_timeout(Duration::from_secs(5))
                .map_err(|_| ThemeIoError::Worker)?;
            Ok(s.file.load()?)
        })?
    };
    observe(&rt, rx)?;
    let notification = owner.handle().observe_close();
    owner.close();
    rt.block_on(notification.wait());
    assert!(ACTIVE.load(Ordering::Acquire));
    assert!(matches!(Quota::take(), Err(ThemeIoError::Busy)));
    release.send(())?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(ThemeIoError::Closed)
    ));
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}

#[test]
fn worker_panic_is_not_a_file_receipt_and_releases_quota() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = rt()?;
    let (_d, owner) = fixture()?;
    let request = {
        let _enter = rt.enter();
        dispatch(&owner.handle(), |_| panic!("fixed theme worker failure"))?
    };
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(ThemeIoError::Worker)
    ));
    assert!(!ACTIVE.load(Ordering::Acquire));
    rt.block_on(async { owner.handle().begin_load()?.ready().await })?;
    Ok(())
}
