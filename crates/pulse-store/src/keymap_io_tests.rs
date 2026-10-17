use super::*;
use crate::keymap::{KeymapAction, KeymapOverride};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    sync::{Mutex, atomic::AtomicUsize, mpsc},
    time::Duration,
};
use tokio::runtime::{Builder, Runtime};
static TESTS: Mutex<()> = Mutex::new(());
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn runtime() -> std::io::Result<Runtime> {
    Builder::new_current_thread()
        .enable_time()
        .max_blocking_threads(1)
        .build()
}
fn fixture(rt: &Runtime) -> Result<(tempfile::TempDir, KeymapIoOwner), Box<dyn std::error::Error>> {
    let directory = tempfile::tempdir()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    let file = KeymapFile::open_private(directory.path().into())?;
    Ok((directory, KeymapIoOwner::new(file, rt.handle().clone())))
}
fn observe<T>(
    rt: &Runtime,
    receiver: oneshot::Receiver<T>,
) -> Result<T, Box<dyn std::error::Error>> {
    Ok(rt.block_on(async { tokio::time::timeout(Duration::from_secs(5), receiver).await })??)
}
struct Guard(Arc<AtomicUsize>);
impl Drop for Guard {
    fn drop(&mut self) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
fn edit() -> KeymapMutation {
    KeymapMutation::Set(KeymapOverride {
        action: KeymapAction::Settings,
        keys: vec!["ctrl+shift+s".into()],
        when: String::new(),
    })
}
#[test]
fn begin_outside_tokio_context_can_load_save_and_reopen() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (directory, owner) = fixture(&rt)?;
    assert!(Handle::try_current().is_err());
    let first = rt.block_on(owner.handle().begin_load()?.ready())?;
    let saved = rt.block_on(
        owner
            .handle()
            .begin_save_retaining(first, edit(), || true, ())?
            .ready(),
    )?;
    assert!(
        saved
            .projection()?
            .bindings
            .iter()
            .any(|binding| binding.action == KeymapAction::Settings)
    );
    let reopened = KeymapFile::open_private(directory.path().into())?;
    assert_eq!(reopened.load()?.projection()?, saved.projection()?);
    Ok(())
}
#[test]
fn active_cancel_keeps_quota_and_guard_until_actual_exit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let drops = Arc::new(AtomicUsize::new(0));
    let (started, start) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = dispatch(&owner.handle(), Guard(drops.clone()), move |state, _| {
        let _sent = started.send(());
        wait.recv_timeout(Duration::from_secs(5))
            .map_err(|_| KeymapIoError::Worker)?;
        Ok(state.file.load()?)
    })?;
    observe(&rt, start)?;
    let cancelled = request.cancel();
    assert!(matches!(
        owner.handle().begin_load(),
        Err(KeymapIoError::Busy)
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    release.send(())?;
    rt.block_on(cancelled.wait_exit())?;
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!ACTIVE.load(Ordering::Acquire));
    rt.block_on(owner.handle().begin_load()?.ready())?;
    Ok(())
}
#[test]
fn queued_cancel_skips_work_and_holds_reservation_until_job_exit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let (started, start) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let blocker = rt.spawn_blocking(move || {
        let _sent = started.send(());
        wait.recv_timeout(Duration::from_secs(5))
    });
    observe(&rt, start)?;
    let work = Arc::new(AtomicUsize::new(0));
    let drops = Arc::new(AtomicUsize::new(0));
    let observed = work.clone();
    let cancelled = dispatch(&owner.handle(), Guard(drops.clone()), move |state, _| {
        observed.fetch_add(1, Ordering::SeqCst);
        Ok(state.file.load()?)
    })?
    .cancel();
    assert!(ACTIVE.load(Ordering::Acquire));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    release.send(())?;
    rt.block_on(blocker)??;
    rt.block_on(cancelled.wait_exit())?;
    assert_eq!(work.load(Ordering::SeqCst), 0);
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn owner_close_rejects_late_receipt_without_releasing_active_credit() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let handle = owner.handle();
    let (started, start) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = dispatch(&handle, (), move |state, _| {
        let _sent = started.send(());
        wait.recv_timeout(Duration::from_secs(5))
            .map_err(|_| KeymapIoError::Worker)?;
        Ok(state.file.load()?)
    })?;
    observe(&rt, start)?;
    owner.close();
    assert!(!handle.is_open());
    assert!(matches!(handle.begin_load(), Err(KeymapIoError::Closed)));
    assert!(ACTIVE.load(Ordering::Acquire));
    release.send(())?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(KeymapIoError::Closed)
    ));
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}
#[test]
fn cancellation_inside_save_rechecks_before_install_and_preserves_file() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (directory, owner) = fixture(&rt)?;
    let expected = rt.block_on(owner.handle().begin_load()?.ready())?;
    let original_bindings = expected.projection()?.bindings;
    let (started, start) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let started = Mutex::new(Some(started));
    let wait = Mutex::new(wait);
    let calls = AtomicUsize::new(0);
    let request = owner.handle().begin_save_retaining(
        expected,
        edit(),
        move || {
            if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                if let Ok(mut start) = started.lock()
                    && let Some(start) = start.take()
                {
                    let _sent = start.send(());
                }
                return wait
                    .lock()
                    .is_ok_and(|wait| wait.recv_timeout(Duration::from_secs(5)).is_ok());
            }
            true
        },
        (),
    )?;
    observe(&rt, start)?;
    let cancelled = request.cancel();
    assert!(matches!(
        owner.handle().begin_load(),
        Err(KeymapIoError::Busy)
    ));
    release.send(())?;
    rt.block_on(cancelled.wait_exit())?;
    assert!(!directory.path().join("keymap.toml").exists());
    assert_eq!(
        rt.block_on(owner.handle().begin_load()?.ready())?
            .projection()?
            .bindings
            .clone(),
        original_bindings
    );
    Ok(())
}
#[test]
fn runtime_shutdown_cannot_release_active_native_guard_or_quota() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let drops = Arc::new(AtomicUsize::new(0));
    let (started, start) = oneshot::channel();
    let (release, wait) = mpsc::sync_channel(1);
    let request = dispatch(&owner.handle(), Guard(drops.clone()), move |state, _| {
        let _sent = started.send(());
        wait.recv_timeout(Duration::from_secs(5))
            .map_err(|_| KeymapIoError::Worker)?;
        Ok(state.file.load()?)
    })?;
    observe(&rt, start)?;
    let cancelled = request.cancel();
    rt.shutdown_background();
    assert!(ACTIVE.load(Ordering::Acquire));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    release.send(())?;
    runtime()?.block_on(cancelled.wait_exit())?;
    assert!(!ACTIVE.load(Ordering::Acquire));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    Ok(())
}
#[test]
fn joint_confirmation_retains_credit_and_guard_when_page_context_is_revoked() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let expected = rt.block_on(owner.handle().begin_load()?.ready())?;
    let (started, start) = oneshot::channel();
    let started = Mutex::new(Some(started));
    let (release, wait) = mpsc::sync_channel(1);
    let wait = Mutex::new(wait);
    let valid = Arc::new(AtomicBool::new(true));
    let context = valid.clone();
    let drops = Arc::new(AtomicUsize::new(0));
    let request = owner.handle().begin_confirm_retaining(
        expected,
        move || {
            if let Ok(mut sender) = started.lock()
                && let Some(sender) = sender.take()
            {
                let _sent = sender.send(());
            }
            if !wait
                .lock()
                .is_ok_and(|wait| wait.recv_timeout(Duration::from_secs(5)).is_ok())
            {
                return false;
            }
            context.load(Ordering::Acquire)
        },
        Guard(drops.clone()),
    )?;
    observe(&rt, start)?;
    valid.store(false, Ordering::Release);
    assert!(matches!(
        owner.handle().begin_load(),
        Err(KeymapIoError::Busy)
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 0);
    release.send(())?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(KeymapIoError::File(KeymapFileError::Authorization))
    ));
    assert_eq!(drops.load(Ordering::SeqCst), 1);
    assert!(!ACTIVE.load(Ordering::Acquire));
    Ok(())
}

#[test]
fn worker_panic_and_foreign_snapshot_never_publish_a_success() -> TestResult {
    let _serial = TESTS.lock().map_err(|_| "serial mutex")?;
    let rt = runtime()?;
    let (_directory, owner) = fixture(&rt)?;
    let request = dispatch(&owner.handle(), (), |_, _| {
        panic!("controlled worker failure")
    })?;
    assert!(matches!(
        rt.block_on(request.ready()),
        Err(KeymapIoError::Worker)
    ));
    assert!(!ACTIVE.load(Ordering::Acquire));
    let (_other_directory, other) = fixture(&rt)?;
    let foreign = rt.block_on(other.handle().begin_load()?.ready())?;
    assert!(!owner.handle().same_source(&other.handle()));
    assert!(matches!(
        rt.block_on(
            owner
                .handle()
                .begin_save_retaining(foreign, edit(), || true, ())?
                .ready()
        ),
        Err(KeymapIoError::File(KeymapFileError::Conflict))
    ));
    Ok(())
}
