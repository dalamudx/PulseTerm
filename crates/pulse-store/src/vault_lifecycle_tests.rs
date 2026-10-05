use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
fn id() -> Uuid {
    Uuid::from_bytes([1; 16])
}
fn proof() -> Result<AuthenticatedDek, CryptoError> {
    AuthenticatedDek::fixture(id())
}
#[test]
fn candidate_requires_confirmation_and_key_use_stops_on_lock() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let ctx = SecretContext::new(id(), "password")?;
    let mut permit = scope.begin()?;
    assert!(matches!(scope.begin(), Err(LifecycleError::Busy)));
    let worker = permit.worker()?;
    assert!(matches!(permit.worker(), Err(LifecycleError::Busy)));
    let mut candidate = worker.finish(proof()?)?;
    assert_eq!(scope.status()?, KeyStatus::Pending);
    assert!(matches!(
        scope.seal(&ctx, b"fixture"),
        Err(KeyUseError::Lifecycle(LifecycleError::Locked))
    ));
    scope.confirm(&mut permit, &mut candidate)?;
    drop(permit);
    assert_eq!(scope.status()?, KeyStatus::Available);
    scope.seal(&ctx, b"fixture")?;
    scope.lock()?;
    scope.lock()?;
    assert_eq!(scope.status()?, KeyStatus::Locked);
    assert!(scope.seal(&ctx, b"fixture").is_err());
    Ok(())
}
#[test]
fn lock_blocks_worker_and_already_returned_candidate() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut first = scope.begin()?;
    let worker = first.worker()?;
    scope.lock()?;
    assert!(matches!(
        worker.finish(proof()?),
        Err(LifecycleError::Stale)
    ));
    let mut current = scope.begin()?;
    let mut candidate = current.worker()?.finish(proof()?)?;
    scope.lock()?;
    assert!(matches!(
        scope.confirm(&mut current, &mut candidate),
        Err(LifecycleError::Stale)
    ));
    Ok(())
}
#[test]
fn drop_revokes_without_touching_new_permit() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut old = scope.begin()?;
    let mut candidate = old.worker()?.finish(proof()?)?;
    drop(old);
    assert_eq!(scope.status()?, KeyStatus::Locked);
    let mut new = scope.begin()?;
    assert!(matches!(
        scope.confirm(&mut new, &mut candidate),
        Err(LifecycleError::Stale)
    ));
    assert_eq!(scope.status()?, KeyStatus::Pending);
    let mut fresh = new.worker()?.finish(proof()?)?;
    scope.confirm(&mut new, &mut fresh)?;
    Ok(())
}
#[test]
fn foreign_scope_close_and_same_id_reconstruction_never_revive_candidate() -> TestResult {
    let a = VaultKeyLifecycle::new(id())?;
    let b = VaultKeyLifecycle::new(id())?;
    let mut permit = a.begin()?;
    let mut candidate = permit.worker()?.finish(proof()?)?;
    assert!(matches!(
        b.confirm(&mut permit, &mut candidate),
        Err(LifecycleError::Stale)
    ));
    assert_eq!(a.status()?, KeyStatus::Pending);
    let mut pb = b.begin()?;
    let worker = pb.worker()?;
    b.close()?;
    assert!(matches!(
        worker.finish(proof()?),
        Err(LifecycleError::Closed)
    ));
    let replacement = VaultKeyLifecycle::new(id())?;
    let mut closed_candidate = UnlockCandidate {
        owner: pb.owner.clone(),
        token: pb.token.clone(),
        confirmed: false,
    };
    assert!(matches!(
        replacement.confirm(&mut pb, &mut closed_candidate),
        Err(LifecycleError::Stale)
    ));
    Ok(())
}
#[test]
fn destroyed_owner_and_wrong_vault_identity_refuse_key() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut permit = scope.begin()?;
    let worker = permit.worker()?;
    drop(scope);
    assert!(matches!(
        worker.finish(proof()?),
        Err(LifecycleError::Closed)
    ));
    let scope = VaultKeyLifecycle::new(id())?;
    let mut p = scope.begin()?;
    assert!(matches!(
        p.worker()?
            .finish(AuthenticatedDek::fixture(Uuid::from_bytes([2; 16]))?),
        Err(LifecycleError::Identity)
    ));
    assert_eq!(scope.status()?, KeyStatus::Pending);
    Ok(())
}
#[test]
fn late_thread_cannot_replace_new_pending_epoch() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut old = scope.begin()?;
    let worker = old.worker()?;
    let (release, receive) = std::sync::mpsc::sync_channel::<()>(1);
    let job = std::thread::spawn(move || {
        receive.recv().map_err(|_| LifecycleError::Closed)?;
        worker.finish(proof().map_err(|_| LifecycleError::Locked)?)
    });
    scope.lock()?;
    drop(old);
    let mut current = scope.begin()?;
    release.send(())?;
    let result = job.join().map_err(|_| "fixed worker panic")?;
    assert!(matches!(result, Err(LifecycleError::Stale)));
    assert_eq!(scope.status()?, KeyStatus::Pending);
    let mut candidate = current.worker()?.finish(proof()?)?;
    scope.confirm(&mut current, &mut candidate)?;
    Ok(())
}
#[test]
fn duplicate_confirm_and_exhausted_lock_do_not_leave_an_active_key() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut permit = scope.begin()?;
    let mut candidate = permit.worker()?.finish(proof()?)?;
    let mut duplicate = UnlockCandidate {
        owner: permit.owner.clone(),
        token: permit.token.clone(),
        confirmed: false,
    };
    scope.confirm(&mut permit, &mut candidate)?;
    assert!(matches!(
        scope.confirm(&mut permit, &mut duplicate),
        Err(LifecycleError::Stale)
    ));
    assert_eq!(scope.status()?, KeyStatus::Available);
    guard(&scope.shared)?.epoch = u64::MAX;
    assert!(matches!(scope.lock(), Err(LifecycleError::Exhausted)));
    let state = scope.shared.lock().map_err(|_| LifecycleError::Closed)?;
    assert!(state.key.is_none());
    assert!(state.closed);
    Ok(())
}
#[test]
fn lock_and_receipt_drop_immediately_remove_scope_owned_candidate_keys() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    let mut permit = scope.begin()?;
    let mut candidate = permit.worker()?.finish(proof()?)?;
    assert!(guard(&scope.shared)?.candidate_key.is_some());
    scope.lock()?;
    assert!(guard(&scope.shared)?.candidate_key.is_none());
    assert!(matches!(
        scope.confirm(&mut permit, &mut candidate),
        Err(LifecycleError::Stale)
    ));
    let mut next = scope.begin()?;
    let receipt = next.worker()?.finish(proof()?)?;
    assert!(guard(&scope.shared)?.candidate_key.is_some());
    drop(receipt);
    assert!(guard(&scope.shared)?.candidate_key.is_none());
    assert_eq!(scope.status()?, KeyStatus::Locked);
    Ok(())
}
#[test]
fn exhaustion_and_poison_clear_key_and_fail_closed() -> TestResult {
    let scope = VaultKeyLifecycle::new(id())?;
    guard(&scope.shared)?.epoch = u64::MAX;
    assert!(matches!(scope.begin(), Err(LifecycleError::Exhausted)));
    assert!(scope.status().is_err());
    let scope = VaultKeyLifecycle::new(id())?;
    let mut p = scope.begin()?;
    let mut c = p.worker()?.finish(proof()?)?;
    scope.confirm(&mut p, &mut c)?;
    let shared = scope.shared.clone();
    let thread = std::thread::spawn(move || {
        let _held = shared.lock();
        panic!("fixed poison fixture");
    });
    assert!(thread.join().is_err());
    assert!(matches!(scope.status(), Err(LifecycleError::Closed)));
    if let Err(poison) = scope.shared.lock() {
        assert!(poison.into_inner().key.is_none());
    }
    Ok(())
}
