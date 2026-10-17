use super::*;
use pulse_store::ssh_route_source::SshRouteIdentity;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

#[test]
fn original_route_sql_read_matches_page_leaf_and_io_scope_without_reserving_operation() -> TestResult
{
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, source, _) = routed_fixture().await?;
        let destination = source
            .hops()
            .last()
            .ok_or("fixture.route-empty")?
            .profile_id();
        let leaf = SshRouteIdentity::PrivateKey(f.identity);
        let other_page = page()?;
        assert!(
            saved(&f, f.identity)?
                .read_route(
                    &other_page.scope,
                    &f.store,
                    destination,
                    leaf,
                    Arc::new(|| true)
                )
                .await
                .is_err()
        );
        assert!(
            saved(&f, f.identity)?
                .read_route(
                    &f.page.scope,
                    &f.store,
                    destination,
                    leaf,
                    Arc::new(|| false)
                )
                .await
                .is_err()
        );
        let outer_profile = source
            .hops()
            .first()
            .ok_or("fixture.route-empty")?
            .profile_id();
        let outer_identity = source
            .hops()
            .first()
            .ok_or("fixture.route-empty")?
            .identity();
        assert!(
            saved(&f, f.identity)?
                .read_route(
                    &f.page.scope,
                    &f.store,
                    outer_profile,
                    outer_identity,
                    Arc::new(|| true)
                )
                .await
                .is_err()
        );
        let mut route = saved(&f, f.identity)?
            .read_route(
                &f.page.scope,
                &f.store,
                destination,
                leaf,
                Arc::new(|| true),
            )
            .await?;
        drop(f.owner.begin(f.page.scope.clone())?);
        let (selection, hop) = route.take_next_selection()?.into_parts();
        assert!(matches!(
            selection,
            crate::vault_owner::RouteHopGesture::Password(_)
        ));
        assert!(hop.password_source().supports_password_selection(
            source.hops()[0].entrance_id(),
            match outer_identity {
                SshRouteIdentity::Password(id) => id,
                _ => return Err("fixture.outer-kind".into()),
            }
        ));
        drop(route);
        f.finish().await
    })
}

#[test]
fn original_route_sql_read_rejects_epoch_change_during_worker_and_late_scope_revocation()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, source, _) = routed_fixture().await?;
        let destination = source
            .hops()
            .last()
            .ok_or("fixture.route-empty")?
            .profile_id();
        let leaf = SshRouteIdentity::PrivateKey(f.identity);
        let owner = f.owner.clone();
        let checks = Arc::new(AtomicUsize::new(0));
        let observed = checks.clone();
        let gate = Arc::new(move || {
            if observed.fetch_add(1, Ordering::AcqRel) == 1 {
                owner.lock().is_ok()
            } else {
                true
            }
        });
        assert!(
            saved(&f, f.identity)?
                .read_route(&f.page.scope, &f.store, destination, leaf, gate)
                .await
                .is_err()
        );
        assert!(checks.load(Ordering::Acquire) >= 2);
        restore_same_dek(&f).await?;
        let live = Arc::new(AtomicBool::new(true));
        let original = live.clone();
        let captured = f.owner.begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(move || original.load(Ordering::Acquire)),
        )?;
        let changed = live.clone();
        let gates = Arc::new(AtomicUsize::new(0));
        let count = gates.clone();
        let io = Arc::new(move || {
            if count.fetch_add(1, Ordering::AcqRel) > 0 {
                changed.store(false, Ordering::Release);
            }
            true
        });
        assert!(
            captured
                .read_route(&f.page.scope, &f.store, destination, leaf, io)
                .await
                .is_err()
        );
        assert!(gates.load(Ordering::Acquire) >= 2);
        let fresh = saved(&f, f.identity)?
            .read_route(
                &f.page.scope,
                &f.store,
                destination,
                leaf,
                Arc::new(|| true),
            )
            .await?;
        drop(fresh);
        f.finish().await
    })
}
