//! Stored startup → root handoff; /usr/bin/true runs in a real PTY without reading user rc.
#![allow(clippy::unwrap_used)]
use super::*;
use pulse_core::TermSize;
use pulse_pty::{
    launch::LocalSessionEnvironment,
    profiles::{ShellCandidate, ShellKind},
};
use pulse_store::profiles::{AutoProfileEdit, AutomaticShell};
use std::{future::Future, os::unix::fs::PermissionsExt};
use tokio::time::{Duration, timeout};

struct Fixture {
    _dir: tempfile::TempDir,
    store: StateStore,
    task: pulse_store::StateStoreTask,
    registry: SessionRegistry,
    window: crate::registry::WindowToken,
    page: PageLaunchScope,
    root: Arc<Mutex<LocalSessionRoot>>,
    candidates: Vec<ShellCandidate>,
    context: LocalSessionEnvironment,
}
impl Fixture {
    async fn new() -> Self {
        let dir = crate::test_tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let (store, task) = StateStore::open(dir.path().join("state.db")).await.unwrap();
        let registry = SessionRegistry::new();
        let window = registry.open_window().unwrap();
        let page = registry.activate_launch_page(window).unwrap();
        let root = Arc::new(Mutex::new(LocalSessionRoot::new(
            registry.clone(),
            Handle::current(),
        )));
        Self {
            _dir: dir,
            store,
            task,
            registry,
            window,
            page,
            root,
            candidates: vec![ShellCandidate::new(ShellKind::Sh, "/usr/bin/true".into()).unwrap()],
            context: LocalSessionEnvironment {
                application_version: "test-root".into(),
                pane: None,
            },
        }
    }
    fn spec(&self) -> StoredProfileLaunch<'_> {
        StoredProfileLaunch {
            candidates: &self.candidates,
            kind: ShellKind::Sh,
            context: &self.context,
            size: TermSize::new(32, 8).unwrap(),
            history: 16,
        }
    }
    async fn start(&self) -> Result<SessionId, LocalStartError> {
        start_stored_page_local(
            &self.registry,
            &self.page,
            &self.store,
            &self.root,
            self.spec(),
        )
        .await
    }
    async fn shutdown(self) {
        self.registry.shutdown().unwrap();
        self.task.shutdown().await.unwrap();
    }
}

#[tokio::test]
async fn hidden_profile_handoff_survives_page_retirement_until_real_eof_consumption() {
    let f = Fixture::new().await;
    f.store
        .edit_auto_profiles(AutoProfileEdit::Visibility {
            shell: AutomaticShell::Sh,
            hidden: true,
        })
        .await
        .unwrap();
    let id = timeout(Duration::from_secs(5), f.start())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(f.root.lock().unwrap().ids(), vec![id]);
    f.registry.retire_window_views(f.window).unwrap();
    f.registry.close_window(f.window).unwrap();
    let owner = f.root.lock().unwrap().take_for_finish(id).unwrap();
    assert_eq!(f.root.lock().unwrap().occupied(), 1);
    let receipt = timeout(Duration::from_secs(5), owner.finish())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(receipt.native.exit_code, 0);
    assert_eq!(f.root.lock().unwrap().collect_reaped(), 1);
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    f.shutdown().await;
}

#[tokio::test]
async fn fresh_tombstone_and_failed_store_are_not_default_launches() {
    let f = Fixture::new().await;
    f.store
        .edit_auto_profiles(AutoProfileEdit::Suppress(AutomaticShell::Sh))
        .await
        .unwrap();
    assert!(matches!(f.start().await, Err(LocalStartError::Profile(_))));
    assert_eq!(f.root.lock().unwrap().occupied(), 0);
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    f.task.cancel();
    assert!(matches!(f.start().await, Err(LocalStartError::Store(_))));
    assert_eq!(f.root.lock().unwrap().occupied(), 0);
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    f.registry.shutdown().unwrap();
    // Cancellation is supervised; no assertion that an aborted worker is a successful read.
    let _ = f.task.shutdown().await;
}

#[tokio::test]
async fn foreign_or_closed_root_rejects_before_even_reading_failed_store() {
    let f = Fixture::new().await;
    let foreign = SessionRegistry::new();
    let root = Arc::new(Mutex::new(LocalSessionRoot::new(
        foreign.clone(),
        Handle::current(),
    )));
    f.task.cancel();
    assert!(matches!(
        start_stored_page_local(&f.registry, &f.page, &f.store, &root, f.spec()).await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    f.root.lock().unwrap().close_all();
    assert!(matches!(
        f.start().await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    foreign.shutdown().unwrap();
    f.registry.shutdown().unwrap();
    let _ = f.task.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn navigation_during_fresh_store_read_never_commits_or_adopts() {
    let f = Fixture::new().await;
    let mut start = Box::pin(f.start());
    assert!(matches!(
        start
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop())),
        std::task::Poll::Pending
    ));
    let _next = f.registry.activate_launch_page(f.window).unwrap();
    assert!(matches!(
        start.await,
        Err(LocalStartError::Registry(RegistryError::Forbidden))
    ));
    assert_eq!(f.root.lock().unwrap().occupied(), 0);
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    f.shutdown().await;
}

#[tokio::test(flavor = "current_thread")]
async fn caller_cancellation_while_reading_does_not_leave_an_unowned_child() {
    let f = Fixture::new().await;
    let mut start = Box::pin(f.start());
    assert!(matches!(
        start
            .as_mut()
            .poll(&mut std::task::Context::from_waker(std::task::Waker::noop())),
        std::task::Poll::Pending
    ));
    drop(start);
    // Ordered worker round-trip fences the cancelled request without sleeping/polling.
    f.store.load_auto_profiles().await.unwrap();
    assert_eq!(f.root.lock().unwrap().occupied(), 0);
    assert_eq!(f.registry.test_registered_session_count().unwrap(), 0);
    f.shutdown().await;
}
