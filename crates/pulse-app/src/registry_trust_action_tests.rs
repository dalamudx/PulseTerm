use super::*;
use std::{
    future::Future,
    sync::atomic::AtomicUsize,
    task::{Context, Poll, Wake, Waker},
};
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn weak_waiter_revocation_does_not_drain_actual_native_reservation() -> TestResult {
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let owner = PageTrustActions::new(registry.clone(), window);
    let scope = page(&registry, window)?;
    let action = owner.begin(scope.clone(), Uuid::now_v7())?;
    let revocation = action.revocation();
    let native = action.authorization();
    revocation.cancel();
    assert!(!native());
    drop(action);
    let mut drain = Box::pin(owner.wait_idle());
    let waker = Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(matches!(drain.as_mut().poll(&mut context), Poll::Pending));
    drop(drain);
    assert!(matches!(
        owner.begin(scope.clone(), Uuid::now_v7()),
        Err(TrustActionError::Busy)
    ));
    drop(native);
    owner.wait_idle().await?;
    let next = owner.begin(scope, Uuid::now_v7())?;
    revocation.cancel();
    assert!(next.is_current());
    drop(next);
    owner.close();
    owner.wait_idle().await?;
    Ok(())
}
fn page(
    registry: &SessionRegistry,
    window: WindowToken,
) -> std::result::Result<Arc<PageLaunchScope>, super::super::RegistryError> {
    Ok(Arc::new(registry.activate_launch_page(window)?))
}
#[tokio::test]
async fn original_page_no_vault_retirement_retains_native_slot_but_not_waiter() -> TestResult {
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let owner = PageTrustActions::new(registry.clone(), window);
    let host = Uuid::now_v7();
    let original = page(&registry, window)?;
    let action = owner.begin(original.clone(), host)?;
    assert_eq!(action.host(), host);
    assert!(action.is_current());
    let native = action.authorization();
    let waiting = action.cancellation();
    let unused = waiting.clone();
    drop(unused);
    assert!(native());
    assert!(matches!(
        owner.begin(original, host),
        Err(TrustActionError::Busy)
    ));
    owner.retire();
    assert!(!native());
    waiting.wait().await;
    let new_page = page(&registry, window)?;
    drop(action);
    assert!(matches!(
        owner.begin(new_page.clone(), host),
        Err(TrustActionError::Busy)
    ));
    drop(native);
    let new_action = owner.begin(new_page.clone(), host)?;
    assert!(new_action.is_current());
    owner.close();
    assert!(!new_action.is_current());
    assert!(registry.check_launch_page(&new_page).is_ok());
    assert!(matches!(
        owner.begin(new_page, host),
        Err(TrustActionError::Denied)
    ));
    Ok(())
}
#[tokio::test]
async fn wrong_window_registry_old_page_owner_drop_and_host_shape_cannot_admit() -> TestResult {
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let scope = page(&registry, window)?;
    let wrong = PageTrustActions::new(SessionRegistry::new(), window);
    assert!(matches!(
        wrong.begin(scope.clone(), Uuid::now_v7()),
        Err(TrustActionError::Denied)
    ));
    let other = registry.open_window()?;
    let owner = PageTrustActions::new(registry.clone(), window);
    assert!(matches!(
        owner.begin(page(&registry, other)?, Uuid::now_v7()),
        Err(TrustActionError::Denied)
    ));
    assert!(matches!(
        owner.begin(scope.clone(), Uuid::new_v4()),
        Err(TrustActionError::Denied)
    ));
    assert!(matches!(
        owner.begin(scope.clone(), Uuid::nil()),
        Err(TrustActionError::Denied)
    ));
    let current = page(&registry, window)?;
    assert!(matches!(
        owner.begin(scope, Uuid::now_v7()),
        Err(TrustActionError::Denied)
    ));
    let action = owner.begin(current, Uuid::now_v7())?;
    let native = action.authorization();
    let waiter = action.cancellation();
    let weak = Arc::downgrade(&owner.inner);
    drop(owner);
    assert!(weak.upgrade().is_none());
    assert!(!native());
    waiter.wait().await;
    drop(action);
    drop(native);
    Ok(())
}
#[tokio::test]
async fn cancellation_metadata_never_owns_admission_and_expired_original_action_stays_denied()
-> TestResult {
    let registry = SessionRegistry::new();
    let window = registry.open_window()?;
    let scope = page(&registry, window)?;
    let owner = PageTrustActions::new(registry.clone(), window);
    let action = owner.begin(scope.clone(), Uuid::now_v7())?;
    let observer = action.cancellation();
    drop(action);
    let next = owner.begin(scope.clone(), Uuid::now_v7())?;
    observer.wait().await;
    assert!(next.is_current());
    drop(next);
    let (cancelled, _) = watch::channel(false);
    let expired = TrustActionOperation {
        token: Arc::new(Token {
            owner: Arc::downgrade(&owner.inner),
            page: scope.clone(),
            host: Uuid::now_v7(),
            expires: Instant::now(),
            live: AtomicBool::new(true),
            cancelled,
        }),
    };
    *owner
        .inner
        .slot
        .lock()
        .map_err(|_| TrustActionError::Unavailable)? = Arc::downgrade(&expired.token);
    let native = expired.authorization();
    assert!(!native());
    expired.cancellation().wait().await;
    drop(expired);
    assert!(matches!(
        owner.begin(scope.clone(), Uuid::now_v7()),
        Err(TrustActionError::Busy)
    ));
    drop(native);
    assert!(owner.begin(scope, Uuid::now_v7())?.is_current());
    Ok(())
}
struct Probe {
    owner: Weak<Inner>,
    wakes: AtomicUsize,
    outside: AtomicBool,
}
impl Wake for Probe {
    fn wake(self: Arc<Self>) {
        self.wake_by_ref();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.wakes.fetch_add(1, Ordering::SeqCst);
        if let Some(owner) = self.owner.upgrade() {
            self.outside.store(
                matches!(
                    owner.slot.try_lock(),
                    Ok(_) | Err(std::sync::TryLockError::Poisoned(_))
                ),
                Ordering::SeqCst,
            );
        }
    }
}
#[tokio::test]
async fn retire_and_poison_recovery_wake_after_owner_lock_release() -> TestResult {
    for poison in [false, true] {
        let registry = SessionRegistry::new();
        let window = registry.open_window()?;
        let scope = page(&registry, window)?;
        let owner = PageTrustActions::new(registry, window);
        let action = owner.begin(scope, Uuid::now_v7())?;
        let probe = Arc::new(Probe {
            owner: Arc::downgrade(&owner.inner),
            wakes: AtomicUsize::new(0),
            outside: AtomicBool::new(false),
        });
        let waker = Waker::from(probe.clone());
        let mut cx = Context::from_waker(&waker);
        let mut future = Box::pin(action.cancellation().wait());
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Pending));
        if poison {
            let inner = owner.inner.clone();
            assert!(
                std::thread::spawn(move || {
                    let _guard = inner.slot.lock().unwrap_or_else(|e| e.into_inner());
                    panic!("fixed fixture poison");
                })
                .join()
                .is_err()
            );
        }
        if poison {
            assert!(!action.is_current());
        }
        owner.retire();
        assert!(probe.wakes.load(Ordering::SeqCst) > 0);
        assert!(probe.outside.load(Ordering::SeqCst));
        assert!(matches!(future.as_mut().poll(&mut cx), Poll::Ready(())));
        assert!(!action.is_current());
    }
    Ok(())
}
