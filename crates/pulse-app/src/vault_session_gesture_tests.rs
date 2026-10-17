use super::*;
use std::sync::atomic::{AtomicBool, Ordering};

#[test]
fn actual_epoch_gesture_is_key_free_single_use_and_cannot_borrow_a_later_unlock() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let p = page()?;
        let foreign = page()?;
        let owner = PageVaultOwner::new(p.registry.clone());
        let other_owner = PageVaultOwner::new(p.registry.clone());
        let selected = (Uuid::now_v7(), Uuid::now_v7());
        let allow = || -> Authorize { Arc::new(|| true) };
        let capture = || {
            owner.begin_password_session_gesture_authorized(
                p.scope.clone(),
                selected.0,
                selected.1,
                allow(),
            )
        };
        assert!(capture().is_err());
        assert!(
            owner
                .begin_password_session_gesture_authorized(
                    foreign.scope.clone(),
                    selected.0,
                    selected.1,
                    allow()
                )
                .is_err()
        );
        let root = tempfile::tempdir()?;
        let dir = pulse_store::state_directory::prepare(&root.path().join("state"))?;
        let (store, worker) = StateStore::open(dir.join("state.db")).await?;
        let empty = store.load_vault_ciphertext(allow()).await?;
        let operation = owner.begin(p.scope.clone())?;
        let (vault, request) = operation.begin_creation(
            PasswordInput::new(b"fixed-gesture-master".to_vec())?,
            empty.clone(),
        )?;
        operation
            .persist_creation(&store, empty, vault, request.ready().await?)
            .await?;
        let source = store.load_vault_ciphertext(allow()).await?;
        assert!(capture().is_err());
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-gesture-master".to_vec())?,
            source.clone(),
        )?;
        let activation = operation
            .confirm_unlock(&store, source.clone(), request.ready().await?)
            .await?;
        // SQL may have enabled the key, but no frontend acceptance means no new gesture.
        assert!(capture().is_err());
        activation.accept()?;
        assert!(
            owner
                .begin_password_session_gesture_authorized(
                    p.scope.clone(),
                    Uuid::nil(),
                    selected.1,
                    allow()
                )
                .is_err()
        );

        let selection = capture()?;
        assert!(selection.matches_selection(selected.0, selected.1));
        assert!(!selection.matches_selection(selected.0, Uuid::now_v7()));
        drop(selection);
        let mut expired = capture()?;
        expired.expires = std::time::Instant::now();
        assert!(
            expired
                .begin_session_authorized(&owner, &p.scope, allow())
                .is_err()
        );
        let gesture = capture()?;
        // Capture holds no owner operation/I/O/CPU/native reservation.
        drop(owner.begin(p.scope.clone())?);
        let action = gesture.begin_session_authorized(&owner, &p.scope, allow())?;
        drop(action);
        assert!(
            capture()?
                .begin_session_authorized(&other_owner, &p.scope, allow())
                .is_err()
        );
        assert!(
            capture()?
                .begin_session_authorized(&owner, &foreign.scope, allow())
                .is_err()
        );
        assert!(
            capture()?
                .begin_session_authorized(&owner, &p.scope, Arc::new(|| false))
                .is_err()
        );
        let cancelled = Arc::new(AtomicBool::new(true));
        let flag = cancelled.clone();
        let gesture = owner.begin_password_session_gesture_authorized(
            p.scope.clone(),
            selected.0,
            selected.1,
            Arc::new(move || flag.load(Ordering::Acquire)),
        )?;
        cancelled.store(false, Ordering::Release);
        assert!(
            gesture
                .begin_session_authorized(&owner, &p.scope, allow())
                .is_err()
        );
        let approved = Arc::new(AtomicBool::new(true));
        let flag = approved.clone();
        let action = capture()?.begin_session_authorized(
            &owner,
            &p.scope,
            Arc::new(move || flag.load(Ordering::Acquire)),
        )?;
        let token = owner
            .inner
            .guard()?
            .pending
            .upgrade()
            .ok_or("fixture.token")?;
        assert!((token.guard)());
        approved.store(false, Ordering::Release);
        assert!(!(token.guard)());
        drop(token);
        drop(action);

        let old = capture()?;
        owner.lock()?;
        let operation = owner.begin(p.scope.clone())?;
        let request = operation.begin_unlock(
            PasswordInput::new(b"fixed-gesture-master".to_vec())?,
            source.clone(),
        )?;
        operation
            .confirm_unlock(&store, source.clone(), request.ready().await?)
            .await?
            .accept()?;
        assert_eq!(
            owner.begin(p.scope.clone())?.status(source.clone())?,
            Some(KeyStatus::Available)
        );
        assert!(
            old.begin_session_authorized(&owner, &p.scope, allow())
                .is_err()
        );
        let stale_page = capture()?;
        p.documents.started(p.window)?;
        owner.retire_page()?;
        let nonce = p.documents.finished(p.window)?;
        p.documents.confirm(p.window, nonce)?;
        let current = p.documents.current(p.window, nonce)?;
        assert!(
            stale_page
                .begin_session_authorized(&owner, &current, allow())
                .is_err()
        );
        let ended = owner.begin_password_session_gesture_authorized(
            current.clone(),
            selected.0,
            selected.1,
            allow(),
        )?;
        owner.close()?;
        assert!(
            ended
                .begin_session_authorized(&owner, &current, allow())
                .is_err()
        );
        drop(store);
        worker.join().await?;
        p.registry.shutdown()?;
        foreign.registry.shutdown()?;
        Ok(())
    })
}
