use super::*;
#[path = "ssh_route_read_tests.rs"]
mod read;
use pulse_store::ssh_route_source::SshRouteSource;

async fn routed_fixture()
-> std::result::Result<(Fixture, SshRouteSource, Uuid), Box<dyn std::error::Error>> {
    let f = Fixture::new().await?;
    f.seal(f.key, SecretKind::SshPrivateKey).await?;
    let password = password_identity(&f).await?;
    let snapshot = f
        .store
        .edit_ssh_hosts(
            f.source().await?.hosts().clone(),
            pulse_store::ssh_hosts::HostEdit::Create(pulse_core::ssh_endpoint::SshEndpoint::new(
                "bastion",
                "jump.invalid",
                22,
                "user",
            )?),
            Arc::new(|| true),
        )
        .await?;
    let entrance = snapshot
        .records()
        .iter()
        .find(|r| r.id() != f.host)
        .ok_or("fixture.bastion")?
        .id();
    let profiles = f
        .store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    let leaf = profiles
        .profiles()
        .find(|p| p.endpoint_id() == f.host)
        .ok_or("fixture.leaf")?
        .metadata()
        .id();
    let via = profiles
        .profiles()
        .find(|p| p.endpoint_id() == entrance)
        .ok_or("fixture.via")?
        .metadata()
        .id();
    let profiles = f
        .store
        .edit_ssh_profile(
            profiles,
            ProfileEdit {
                id: via,
                label: "bastion".into(),
                username: "user".into(),
                identity: Some(password),
            },
            Arc::new(|| true),
        )
        .await?;
    f.store
        .edit_ssh_profile_jump(profiles, leaf, Some(via), Arc::new(|| true))
        .await?;
    let route = f
        .store
        .load_ssh_route_source(
            leaf,
            SshRouteIdentity::PrivateKey(f.identity),
            Arc::new(|| true),
        )
        .await?;
    Ok((f, route, password))
}

#[test]
fn one_original_route_click_orders_roles_pin_successor_and_cancel_drop_revoke_issued_intents()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, source, password) = routed_fixture().await?;
        assert!(saved(&f, password)?.select_route(source.clone()).is_err());
        let mut route = saved(&f, f.identity)?.select_route(source.clone())?;
        assert!(
            route
                .accept_first_pin(source.hop_source(0)?.password_source())
                .is_err()
        );
        let (outer, outer_source) = route.take_next_selection()?.into_parts();
        let outer = match outer {
            crate::vault_owner::RouteHopGesture::Password(g) => g,
            _ => return Err("fixture.outer-kind".into()),
        };
        let outer_hop = &source.hops()[0];
        let pin = pulse_store::ssh_trust::TrustRecord::new(
            pulse_store::ssh_trust::TrustTarget::new(outer_hop.endpoint().target().host(), 22)?,
            "ssh-ed25519 AAAA".into(),
        )?;
        let updated = f
            .store
            .approve_first_ssh_pin(
                outer_source.password_source().clone(),
                outer_hop.entrance_id(),
                pin,
                Arc::new(|| true),
            )
            .await?;
        let current = route.accept_first_pin(&updated)?;
        // Neither a known-pin replacement nor the same reply twice can refresh the old click.
        assert!(route.accept_first_pin(&updated).is_err());
        let (final_selection, final_source) = route.take_next_selection()?.into_parts();
        assert!(matches!(
            final_selection,
            crate::vault_owner::RouteHopGesture::PrivateKey(_)
        ));
        assert!(final_source.supports_private_key_selection(f.host, f.identity));
        assert!(route.take_next_selection().is_err());
        f.store
            .validate_ssh_key_target_source(final_source, Arc::new(|| true))
            .await?;
        let intent = outer
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, current.password_source().clone())
            .await?;
        let guard = intent.authorization();
        assert!(guard());
        route.cancel();
        assert!(!guard());
        assert!(route.take_next_selection().is_err());
        assert!(route.accept_first_pin(&updated).is_err());
        drop(intent);
        assert!(matches!(saved(&f, f.identity), Err(VaultOwnerError::Busy)));
        drop(guard);
        let current_route = source.after_first_pin(0, &updated)?;
        let mut route = saved(&f, f.identity)?.select_route(current_route)?;
        let (outer, current) = route.take_next_selection()?.into_parts();
        let outer = match outer {
            crate::vault_owner::RouteHopGesture::Password(g) => g,
            _ => return Err("fixture.outer-kind".into()),
        };
        let intent = outer
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, current.password_source().clone())
            .await?;
        let guard = intent.authorization();
        assert!(guard());
        drop(route);
        assert!(!guard());
        drop(intent);
        f.finish().await
    })
}

#[test]
fn route_later_hops_and_issued_gestures_cannot_capture_a_new_epoch_after_same_dek_unlock()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, source, _) = routed_fixture().await?;
        let mut route = saved(&f, f.identity)?.select_route(source.clone())?;
        let (issued, _) = route.take_next_selection()?.into_parts();
        restore_same_dek(&f).await?;
        assert!(route.take_next_selection().is_err());
        let issued = match issued {
            crate::vault_owner::RouteHopGesture::Password(g) => g,
            _ => return Err("fixture.outer-kind".into()),
        };
        assert!(
            issued
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        // A fresh explicit click can use the new available epoch; the old route cannot.
        let mut fresh = saved(&f, f.identity)?.select_route(source)?;
        assert!(fresh.take_next_selection().is_ok());
        drop(fresh);
        drop(route);
        f.finish().await
    })
}
