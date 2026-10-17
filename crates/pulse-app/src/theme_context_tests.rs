use super::*;
use pulse_store::{
    theme::{
        AccentChoice, NeutralScale, ThemeAppearance, ThemeImportFormat, UiThemeBase, import_theme,
    },
    theme_file::{ThemeFile, ThemeFileError},
    theme_io::ThemeIoOwner,
};
use std::{
    os::unix::fs::PermissionsExt,
    sync::{Mutex, mpsc},
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};
use uuid::Uuid;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
pub(crate) static TESTS: Mutex<()> = Mutex::new(());
pub(crate) fn runtime() -> std::io::Result<Runtime> {
    Builder::new_multi_thread()
        .worker_threads(1)
        .max_blocking_threads(1)
        .enable_all()
        .build()
}
pub(crate) fn document(
    name: &str,
) -> std::result::Result<ThemeDocument, Box<dyn std::error::Error>> {
    let keys = [
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
    let fields = keys
        .into_iter()
        .map(|key| format!("\"{key}\":\"#112233\""))
        .collect::<Vec<_>>()
        .join(",");
    let import = import_theme(
        ThemeImportFormat::WindowsTerminal,
        &format!("{{\"name\":\"{name}\",{fields}}}"),
        None,
    )?;
    Ok(ThemeDocument::from_import(
        &import,
        ThemeAppearance::Dark,
        UiThemeBase::new(NeutralScale::Slate, AccentChoice::parse("cyan")?),
    )?)
}
fn pending_revocation(future: std::pin::Pin<&mut impl std::future::Future<Output = ()>>) {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.poll(&mut cx).is_pending());
}
fn revoked(future: std::pin::Pin<&mut impl std::future::Future<Output = ()>>) {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(future.poll(&mut cx).is_ready());
}
#[test]
fn revocation_observers_noop_aba_late_registration_and_coalescing() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let a = Fixture::new("signal-a")?;
    let b = Fixture::new("signal-b")?;
    let owner = a.context()?;
    let h = owner.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let mut observer = Box::pin(p.observe_revocation().wait());
    pending_revocation(observer.as_mut());
    owner.select(a.io.handle())?;
    pending_revocation(observer.as_mut());
    owner.select(b.io.handle())?;
    owner.select(a.io.handle())?;
    for _ in 0..1000 {
        owner.invalidate()?;
    }
    revoked(observer.as_mut());
    revoked(Box::pin(p.observe_revocation().wait()).as_mut());
    assert!(!h.is_current(&p));
    assert!(a.io.handle().is_open() && b.io.handle().is_open());
    Ok(())
}
#[test]
fn revocation_new_publication_revokes_old_without_consuming_io_slot() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let f = Fixture::new("replace-signal")?;
    let o = f.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let mut old = Box::pin(p.observe_revocation().wait());
    pending_revocation(old.as_mut());
    let next = h.confirm(load(&rt, &h)?)?;
    revoked(old.as_mut());
    let mut latest = Box::pin(next.observe_revocation().wait());
    pending_revocation(latest.as_mut());
    assert!(h.is_current(&next));
    drop(latest);
    assert!(h.is_current(&next));
    h.confirm(load(&rt, &h)?)?;
    Ok(())
}
#[test]
fn revocation_owner_drop_is_weak_and_does_not_close_shared_source() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let f = Fixture::new("weak-signal")?;
    let o = f.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let mut observer = Box::pin(p.observe_revocation().wait());
    pending_revocation(observer.as_mut());
    drop(o);
    assert!(h.state.upgrade().is_none());
    revoked(observer.as_mut());
    assert!(f.io.handle().is_open());
    let next = f.context()?;
    next.handle().confirm(load(&rt, &next.handle())?)?;
    Ok(())
}
#[test]
fn revocation_source_close_without_context_signal_is_observed() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let f = Fixture::new("source-close")?;
    let o = f.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let mut observer = Box::pin(p.observe_revocation().wait());
    pending_revocation(observer.as_mut());
    f.io.close();
    revoked(observer.as_mut());
    revoked(Box::pin(p.observe_revocation().wait()).as_mut());
    assert!(!h.is_current(&p));
    Ok(())
}
#[test]
fn revocation_both_counter_exhaustion_paths_wake_publication_observers() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let f = Fixture::new("counter-signal")?;
    for epoch in [true, false] {
        let o = f.context()?;
        let h = o.handle();
        let p = h.confirm(load(&rt, &h)?)?;
        let mut observer = Box::pin(p.observe_revocation().wait());
        pending_revocation(observer.as_mut());
        {
            let mut s = o.state.lock().map_err(|_| "test state")?;
            if epoch {
                s.epoch = u64::MAX;
            } else {
                s.next = u64::MAX;
            }
        }
        if epoch {
            assert!(matches!(o.invalidate(), Err(ThemeContextError::Exhausted)));
        } else {
            assert!(matches!(h.begin_load(), Err(ThemeContextError::Exhausted)));
        }
        revoked(observer.as_mut());
        assert!(matches!(h.last_good(), Err(ThemeContextError::Closed)));
    }
    Ok(())
}
#[test]
fn revocation_poisoned_context_close_wakes_without_restoring_authority() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "test mutex")?;
    let rt = runtime()?;
    let f = Fixture::new("poison-signal")?;
    let o = f.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let mut observer = Box::pin(p.observe_revocation().wait());
    pending_revocation(observer.as_mut());
    let poison = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let Ok(_state) = o.state.lock() else {
            return;
        };
        panic!("fixed context poison");
    }));
    assert!(poison.is_err());
    o.close();
    revoked(observer.as_mut());
    assert!(!h.is_current(&p));
    assert!(matches!(h.begin_load(), Err(ThemeContextError::State)));
    assert!(f.io.handle().is_open());
    Ok(())
}

struct Fixture {
    dir: tempfile::TempDir,
    io: ThemeIoOwner,
    id: Uuid,
}
impl Fixture {
    fn new(name: &str) -> std::result::Result<Self, Box<dyn std::error::Error>> {
        let dir = tempfile::tempdir()?;
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700))?;
        let id = Uuid::new_v4();
        let file = ThemeFile::open_private(dir.path().into(), id)?;
        file.save(&file.load()?, &document(name)?)?;
        Ok(Self {
            dir,
            io: ThemeIoOwner::new(file),
            id,
        })
    }
    fn context(&self) -> Result<ThemeContextOwner> {
        ThemeContextOwner::new(self.io.handle())
    }
}
fn load(rt: &Runtime, h: &ThemeContextHandle) -> Result<ThemeCandidate> {
    rt.block_on(async { h.begin_load()?.ready().await })
}
#[test]
fn read_requires_explicit_confirmation_and_exact_publication_token() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("first")?;
    let o = f.context()?;
    let h = o.handle();
    let c = load(&rt, &h)?;
    assert!(h.last_good()?.is_none());
    assert!(matches!(
        rt.block_on(async { h.begin_load() }),
        Err(ThemeContextError::Busy)
    ));
    let p = h.confirm(c)?;
    assert_eq!(p.document().name(), "first");
    assert!(h.is_current(&p));
    o.select(f.io.handle())?;
    assert!(h.is_current(&p));
    let q = h.confirm(load(&rt, &h)?)?;
    assert!(!h.is_current(&p));
    assert!(h.is_current(&q));
    Ok(())
}
#[test]
fn foreign_context_same_source_cannot_publish_and_drop_clears_slot() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("same")?;
    let a = f.context()?;
    let b = f.context()?;
    let h = a.handle();
    let c = load(&rt, &h)?;
    assert!(matches!(
        b.handle().confirm(c),
        Err(ThemeContextError::Foreign)
    ));
    assert!(h.last_good()?.is_none());
    let next = load(&rt, &h)?;
    drop(next);
    let p = h.confirm(load(&rt, &h)?)?;
    assert!(h.is_current(&p));
    assert!(!b.handle().is_current(&p));
    Ok(())
}
#[test]
fn aba_and_same_uuid_replacement_do_not_revive_candidates() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let a = Fixture::new("a")?;
    let b = Fixture::new("b")?;
    let o = a.context()?;
    let h = o.handle();
    let c = load(&rt, &h)?;
    o.select(b.io.handle())?;
    o.select(a.io.handle())?;
    assert!(matches!(h.confirm(c), Err(ThemeContextError::Stale)));
    let c = load(&rt, &h)?;
    let replacement = ThemeIoOwner::new(ThemeFile::open_private(a.dir.path().into(), a.id)?);
    o.select(replacement.handle())?;
    assert!(matches!(h.confirm(c), Err(ThemeContextError::Stale)));
    let p = h.confirm(load(&rt, &h)?)?;
    assert_eq!(p.document().name(), "a");
    Ok(())
}
#[test]
fn invalid_refresh_keeps_stale_last_good_but_selection_clears_it() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let a = Fixture::new("a")?;
    let b = Fixture::new("b")?;
    let o = a.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    o.invalidate()?;
    std::fs::write(
        a.dir.path().join(format!("{}.theme.toml", a.id)),
        "invalid [",
    )?;
    assert!(matches!(
        load(&rt, &h),
        Err(ThemeContextError::Io(ThemeIoError::File(
            ThemeFileError::Document(_)
        )))
    ));
    assert_eq!(
        h.last_good()?.ok_or("last-good lost")?.document().name(),
        "a"
    );
    assert!(!h.is_current(&p));
    o.select(b.io.handle())?;
    assert!(h.last_good()?.is_none());
    let q = h.confirm(load(&rt, &h)?)?;
    assert_eq!(q.document().name(), "b");
    Ok(())
}
#[test]
fn missing_is_not_default_and_invalid_selection_preserves_current() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("ok")?;
    let o = f.context()?;
    let h = o.handle();
    let p = h.confirm(load(&rt, &h)?)?;
    let unavailable = ThemeIoOwner::new(ThemeFile::open_private(
        f.dir.path().into(),
        Uuid::new_v4(),
    )?);
    unavailable.close();
    assert!(matches!(
        o.select(unavailable.handle()),
        Err(ThemeContextError::Io(ThemeIoError::Closed))
    ));
    assert!(h.is_current(&p));
    let absent = ThemeIoOwner::new(ThemeFile::open_private(
        f.dir.path().into(),
        Uuid::new_v4(),
    )?);
    o.select(absent.handle())?;
    assert!(matches!(load(&rt, &h), Err(ThemeContextError::Missing)));
    assert!(h.last_good()?.is_none());
    Ok(())
}
#[test]
fn cancelled_queued_io_retains_context_slot_through_real_exit() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("queued")?;
    let o = f.context()?;
    let h = o.handle();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = rt.spawn_blocking(move || {
        let _sent = entered.send(());
        let _released = wait.recv();
    });
    started.recv_timeout(Duration::from_secs(5))?;
    let req = rt.block_on(async { h.begin_load() })?;
    o.invalidate()?;
    let cancel = req.cancel();
    assert!(matches!(
        rt.block_on(async { h.begin_load() }),
        Err(ThemeContextError::Busy)
    ));
    release.send(())?;
    rt.block_on(async {
        blocker.await?;
        cancel.join().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    let p = h.confirm(load(&rt, &h)?)?;
    assert!(h.is_current(&p));
    Ok(())
}
#[test]
fn dropped_ready_timeout_preserves_slot_and_does_not_publish() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("timeout")?;
    let o = f.context()?;
    let h = o.handle();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = rt.spawn_blocking(move || {
        let _sent = entered.send(());
        let _released = wait.recv();
    });
    started.recv_timeout(Duration::from_secs(5))?;
    let req = rt.block_on(async { h.begin_load() })?;
    let ThemeReadRequest {
        receiver,
        supervisor,
    } = req;
    assert!(
        rt.block_on(async { tokio::time::timeout(Duration::from_millis(1), receiver).await })
            .is_err()
    );
    assert!(matches!(
        rt.block_on(async { h.begin_load() }),
        Err(ThemeContextError::Busy)
    ));
    release.send(())?;
    rt.block_on(async {
        blocker.await?;
        supervisor.await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    assert!(h.last_good()?.is_none());
    drop(load(&rt, &h)?);
    Ok(())
}
#[test]
fn closed_source_and_context_drop_cannot_publish_or_close_shared_io() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("shared")?;
    let o = f.context()?;
    let h = o.handle();
    let c = load(&rt, &h)?;
    drop(o);
    assert!(matches!(h.confirm(c), Err(ThemeContextError::Closed)));
    assert!(f.io.handle().is_open());
    rt.block_on(async { f.io.handle().begin_load()?.ready().await })?;
    let o = f.context()?;
    let h = o.handle();
    let c = load(&rt, &h)?;
    f.io.close();
    assert!(matches!(
        h.confirm(c),
        Err(ThemeContextError::Io(ThemeIoError::Closed))
    ));
    assert!(matches!(
        ThemeContextOwner::new(f.io.handle()),
        Err(ThemeContextError::Io(ThemeIoError::Closed))
    ));
    Ok(())
}
#[test]
fn close_after_read_and_drop_unconfirmed_candidate_cannot_publish() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("close")?;
    let o = f.context()?;
    let h = o.handle();
    drop(load(&rt, &h)?);
    assert!(h.last_good()?.is_none());
    let c = load(&rt, &h)?;
    o.close();
    assert!(matches!(h.confirm(c), Err(ThemeContextError::Closed)));
    assert!(f.io.handle().is_open());
    Ok(())
}
#[test]
fn source_closed_before_queued_completion_returns_exact_closed_without_publication() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("queued-close")?;
    let o = f.context()?;
    let h = o.handle();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = rt.spawn_blocking(move || {
        let _sent = entered.send(());
        let _released = wait.recv();
    });
    started.recv_timeout(Duration::from_secs(5))?;
    let req = rt.block_on(async { h.begin_load() })?;
    f.io.close();
    release.send(())?;
    let result = rt.block_on(async {
        blocker.await.map_err(|_| ThemeContextError::Worker)?;
        req.ready().await
    });
    assert!(matches!(
        result,
        Err(ThemeContextError::Io(ThemeIoError::Closed))
    ));
    assert!(h.last_good()?.is_none());
    Ok(())
}
#[test]
fn global_busy_does_not_consume_second_context_slot_or_token() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let a = Fixture::new("one")?;
    let b = Fixture::new("two")?;
    let oa = a.context()?;
    let ob = b.context()?;
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = rt.spawn_blocking(move || {
        let _sent = entered.send(());
        let _released = wait.recv();
    });
    started.recv_timeout(Duration::from_secs(5))?;
    let request = rt.block_on(async { oa.handle().begin_load() })?;
    assert!(matches!(
        rt.block_on(async { ob.handle().begin_load() }),
        Err(ThemeContextError::Io(ThemeIoError::Busy))
    ));
    {
        let s = ob.state.lock().map_err(|_| "test context mutex failed")?;
        assert!(s.pending.is_none());
        assert_eq!(s.next, 0);
    }
    let cancellation = request.cancel();
    release.send(())?;
    rt.block_on(async {
        blocker.await?;
        cancellation.join().await?;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    drop(load(&rt, &ob.handle())?);
    Ok(())
}
#[test]
fn runtime_shutdown_cancelled_queue_releases_slot_without_publication() -> TestResult {
    let _serial = TESTS.lock()?;
    let rt = runtime()?;
    let f = Fixture::new("shutdown")?;
    let o = f.context()?;
    let h = o.handle();
    let (entered, started) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let blocker = rt.spawn_blocking(move || {
        let _sent = entered.send(());
        let _released = wait.recv();
    });
    started.recv_timeout(Duration::from_secs(5))?;
    let (notice, exited) = tokio::sync::oneshot::channel();
    o.state
        .lock()
        .map_err(|_| "test context mutex failed")?
        .slot_observer = Some(notice);
    let ThemeReadRequest {
        receiver,
        supervisor,
    } = rt.block_on(async { h.begin_load() })?;
    rt.shutdown_timeout(Duration::ZERO);
    let replacement = runtime()?;
    assert!(replacement.block_on(supervisor).is_err());
    // Tokio shutdown cancels this not-yet-entered blocking job; observe actual last-guard Drop.
    replacement.block_on(async {
        tokio::time::timeout(Duration::from_secs(5), exited).await??;
        Ok::<_, Box<dyn std::error::Error>>(())
    })?;
    assert!(
        o.state
            .lock()
            .map_err(|_| "test context mutex failed")?
            .pending
            .is_none()
    );
    release.send(())?;
    replacement.block_on(blocker)?;
    assert!(replacement.block_on(receiver).is_err());
    assert!(h.last_good()?.is_none());
    let current = h.confirm(load(&replacement, &h)?)?;
    assert!(h.is_current(&current));
    Ok(())
}
#[test]
fn runtime_and_counter_exhaustion_fail_without_enqueueing() -> TestResult {
    let _serial = TESTS.lock()?;
    let f = Fixture::new("runtime")?;
    let o = f.context()?;
    let h = o.handle();
    assert!(matches!(
        h.begin_load(),
        Err(ThemeContextError::Io(ThemeIoError::Runtime))
    ));
    assert!(
        o.state
            .lock()
            .map_err(|_| "test context mutex failed")?
            .pending
            .is_none()
    );
    let rt = runtime()?;
    o.state
        .lock()
        .map_err(|_| "test context mutex failed")?
        .next = u64::MAX;
    assert!(matches!(
        rt.block_on(async { h.begin_load() }),
        Err(ThemeContextError::Exhausted)
    ));
    assert!(matches!(h.last_good(), Err(ThemeContextError::Closed)));
    let other = f.context()?;
    other
        .state
        .lock()
        .map_err(|_| "test context mutex failed")?
        .epoch = u64::MAX;
    assert!(matches!(
        other.invalidate(),
        Err(ThemeContextError::Exhausted)
    ));
    assert!(matches!(
        other.handle().last_good(),
        Err(ThemeContextError::Closed)
    ));
    Ok(())
}
