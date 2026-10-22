use super::*;
use std::{
    future::Future,
    sync::{Weak, atomic::AtomicUsize},
    task::{Context, Wake, Waker},
};
struct Reenter {
    owner: Weak<Inner>,
    token: Weak<Token>,
    outside: AtomicBool,
    count: AtomicUsize,
}
impl Wake for Reenter {
    fn wake(self: Arc<Self>) {
        let authority = self.owner.upgrade().is_none_or(|o| {
            !matches!(o.state.try_lock(), Err(std::sync::TryLockError::WouldBlock))
        });
        let candidate = self.token.upgrade().is_none_or(|t| {
            !matches!(
                t.revocation.try_lock(),
                Err(std::sync::TryLockError::WouldBlock)
            )
        });
        self.outside
            .store(authority && candidate, Ordering::Release);
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}
fn waker(owner: &PageVaultOwner, operation: &PageVaultOperation) -> (Arc<Reenter>, Waker) {
    let tracker = Arc::new(Reenter {
        owner: Arc::downgrade(&owner.inner),
        token: Arc::downgrade(&operation.token),
        outside: AtomicBool::new(false),
        count: AtomicUsize::new(0),
    });
    (tracker.clone(), Waker::from(tracker))
}
#[test]
fn explicit_cancel_retire_lock_close_and_operation_drop_wake_outside_locks_without_releasing_native_slot()
-> TestResult {
    for action in 0..5 {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let operation = owner.begin(p.scope.clone())?;
        let held = operation.authorization();
        let cancel = operation.cancellation();
        let (tracker, waker) = waker(&owner, &operation);
        let mut cx = Context::from_waker(&waker);
        let mut waiting = Box::pin(cancel.wait_cancelled());
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        match action {
            0 => cancel.cancel(),
            1 => owner.retire_page()?,
            2 => owner.lock()?,
            3 => owner.close()?,
            _ => {}
        };
        drop(operation);
        assert!(tracker.count.load(Ordering::SeqCst) > 0);
        assert!(tracker.outside.load(Ordering::Acquire));
        assert!(waiting.as_mut().poll(&mut cx).is_ready());
        assert!(!held());
        if action != 3 {
            assert!(matches!(
                owner.begin(p.scope.clone()),
                Err(VaultOwnerError::Busy)
            ));
        }
        drop(held);
        if action != 3 {
            let next = owner.begin(p.scope.clone())?;
            cancel.cancel();
            assert!((next.authorization())());
            drop(next);
        }
        owner.close()?;
        p.registry.shutdown()?;
    }
    Ok(())
}
#[test]
fn dropped_waiter_does_not_cancel_and_metadata_observer_never_keeps_owner_token_or_capacity()
-> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let operation = owner.begin(p.scope.clone())?;
    let cancel = operation.cancellation();
    let token = Arc::downgrade(&operation.token);
    let root = Arc::downgrade(&owner.inner);
    let (_, wake) = waker(&owner, &operation);
    let mut cx = Context::from_waker(&wake);
    let mut waiting = Box::pin(cancel.wait_cancelled());
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    drop(waiting);
    assert!((operation.authorization())());
    let mut waiting = Box::pin(cancel.wait_cancelled());
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    drop(operation);
    assert!(token.upgrade().is_none());
    assert!(waiting.as_mut().poll(&mut cx).is_ready());
    let next = owner.begin(p.scope.clone())?;
    drop(next);
    drop(owner);
    assert!(root.upgrade().is_none());
    p.registry.shutdown()?;
    Ok(())
}
#[test]
fn owner_drop_with_actual_predicate_retained_wakes_old_receipt_without_retaining_controller()
-> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let operation = owner.begin(p.scope.clone())?;
    let held = operation.authorization();
    let cancel = operation.cancellation();
    let (tracker, wake) = waker(&owner, &operation);
    let mut cx = Context::from_waker(&wake);
    let mut waiting = Box::pin(cancel.wait_cancelled());
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    drop(owner);
    assert!(tracker.count.load(Ordering::SeqCst) > 0);
    assert!(tracker.outside.load(Ordering::Acquire));
    assert!(waiting.as_mut().poll(&mut cx).is_ready());
    assert!(!held());
    drop(operation);
    drop(held);
    p.registry.shutdown()?;
    Ok(())
}
#[test]
fn poisoned_authority_still_wakes_after_recovering_guard_is_released() -> TestResult {
    let p = page()?;
    let owner = PageVaultOwner::new(p.registry.clone());
    let operation = owner.begin(p.scope.clone())?;
    let cancel = operation.cancellation();
    let (tracker, wake) = waker(&owner, &operation);
    let mut cx = Context::from_waker(&wake);
    let mut waiting = Box::pin(cancel.wait_cancelled());
    assert!(waiting.as_mut().poll(&mut cx).is_pending());
    let poison = owner.clone();
    assert!(
        std::thread::spawn(move || {
            let _guard = poison.inner.state.lock().unwrap_or_else(|p| p.into_inner());
            panic!("fixture.poison");
        })
        .join()
        .is_err()
    );
    assert!(owner.lock().is_err());
    assert!(tracker.count.load(Ordering::SeqCst) > 0);
    assert!(tracker.outside.load(Ordering::Acquire));
    assert!(waiting.as_mut().poll(&mut cx).is_ready());
    drop(operation);
    p.registry.shutdown()?;
    Ok(())
}
#[cfg(unix)]
#[test]
fn foreign_store_binding_closes_and_wakes_under_original_page_without_callbacks_in_source_mutex()
-> TestResult {
    runtime()?.block_on(async {
        let p = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let operation = owner.begin(p.scope.clone())?;
        let a = crate::test_tempdir()?;
        let b = crate::test_tempdir()?;
        let a_dir = pulse_store::state_directory::prepare(&a.path().join("state"))?;
        let b_dir = pulse_store::state_directory::prepare(&b.path().join("state"))?;
        let (store, task) = StateStore::open(a_dir.join("state.db")).await?;
        let (other, other_task) = StateStore::open(b_dir.join("state.db")).await?;
        let original = store.load_vault_ciphertext(Arc::new(|| true)).await?;
        assert!(operation.bind(original)?.is_none());
        let cancel = operation.cancellation();
        let (tracker, wake) = waker(&owner, &operation);
        let mut cx = Context::from_waker(&wake);
        let mut waiting = Box::pin(cancel.wait_cancelled());
        assert!(waiting.as_mut().poll(&mut cx).is_pending());
        let foreign = other.load_vault_ciphertext(Arc::new(|| true)).await?;
        assert!(operation.bind(foreign).is_err());
        assert!(tracker.count.load(Ordering::SeqCst) > 0);
        assert!(tracker.outside.load(Ordering::Acquire));
        assert!(waiting.as_mut().poll(&mut cx).is_ready());
        drop(operation);
        drop(store);
        drop(other);
        task.join().await?;
        other_task.join().await?;
        p.registry.shutdown()?;
        Ok(())
    })
}
