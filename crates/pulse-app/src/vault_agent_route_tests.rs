//! Permission checks only; real peer and signing evidence comes from the window probe.
use super::*;
#[path = "vault_agent_route_joint_tests.rs"]
mod joint;
use crate::vault_owner::RouteHopGesture;
use pulse_store::{ssh_profiles::ProfileEdit, ssh_route_source::SshRouteIdentity};

#[test]
fn agent_route_intermediate_cannot_open_terminal_cancel_keeps_slot_and_reunlock_revokes_next_hop()
-> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let (f, agent) = agent_fixture().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let hosts = f
            .store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                pulse_store::ssh_hosts::HostEdit::Create(
                    pulse_core::ssh_endpoint::SshEndpoint::new(
                        "Agent bastion",
                        "agent-jump.invalid",
                        22,
                        "user",
                    )?,
                ),
                Arc::new(|| true),
            )
            .await?;
        let outer = hosts
            .records()
            .iter()
            .find(|r| r.id() != f.host)
            .ok_or("fixture.outer")?
            .id();
        let profiles = f
            .store
            .load_ssh_profiles_authorized(Arc::new(|| true))
            .await?;
        let leaf_profile = profiles
            .profiles()
            .find(|p| p.endpoint_id() == f.host)
            .ok_or("fixture.leaf")?
            .metadata()
            .id();
        let outer_profile = profiles
            .profiles()
            .find(|p| p.endpoint_id() == outer)
            .ok_or("fixture.outer-profile")?
            .metadata()
            .id();
        let profiles = f
            .store
            .edit_ssh_profile(
                profiles,
                ProfileEdit {
                    id: outer_profile,
                    label: "Agent bastion".into(),
                    username: "user".into(),
                    identity: Some(agent),
                },
                Arc::new(|| true),
            )
            .await?;
        f.store
            .edit_ssh_profile_jump(
                profiles,
                leaf_profile,
                Some(outer_profile),
                Arc::new(|| true),
            )
            .await?;
        let source = f
            .store
            .load_ssh_route_source(
                leaf_profile,
                SshRouteIdentity::Agent(agent),
                Arc::new(|| true),
            )
            .await?;
        joint::check(&f, agent, &source).await?;
        let original = || {
            f.owner.begin_routed_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                agent,
                Arc::new(|| true),
            )
        };
        let mut route = original()?.select_route(source.clone())?;
        let (issued, hop) = route.take_next_selection()?.into_parts();
        let issued = match issued {
            RouteHopGesture::Agent(g) => g,
            _ => return Err("fixture.agent-kind".into()),
        };
        let intent = issued
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, hop)
            .await?;
        let guard = intent.authorization();
        assert!(guard());
        let calls = Arc::new(AtomicUsize::new(0));
        let mut sink = RejectUnexpectedSign {
            calls: calls.clone(),
        };
        let mut started =
            Some(ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?);
        assert!(
            intent
                .authenticate_terminal(&f.store, &mut sink, &mut started)
                .await
                .is_err()
        );
        assert_eq!(calls.load(Ordering::Acquire), 0);
        started
            .take()
            .ok_or("fixture.start")?
            .finish_unregistered(async { Ok(()) })
            .join()
            .await?;
        route.cancel();
        assert!(!guard());
        assert!(route.take_next_selection().is_err());
        drop(intent);
        assert!(matches!(
            f.owner.begin(f.page.scope.clone()),
            Err(VaultOwnerError::Busy)
        ));
        drop(guard);
        let mut fresh = original()?.select_route(source.clone())?;
        let (issued, _) = fresh.take_next_selection()?.into_parts();
        let issued = match issued {
            RouteHopGesture::Agent(g) => g,
            _ => return Err("fixture.agent-kind".into()),
        };
        restore_same_dek(&f).await?;
        assert!(fresh.take_next_selection().is_err());
        assert!(
            issued
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
                .is_err()
        );
        drop(fresh);
        drop(route);
        let mut current = original()?.select_route(source)?;
        assert!(matches!(
            current.take_next_selection()?.into_parts().0,
            RouteHopGesture::Agent(_)
        ));
        drop(current);
        f.finish().await
    })
}
