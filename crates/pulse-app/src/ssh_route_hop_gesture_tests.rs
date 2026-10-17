use super::*;
#[path = "ssh_route_capture_tests.rs"]
mod capture;
#[path = "ssh_route_interactive_tests.rs"]
mod interactive;
#[path = "ssh_route_key_interactive_tests.rs"]
mod key_interactive;
use pulse_store::{ssh_profiles::ProfileEdit, ssh_route_source::SshRouteIdentity};

#[test]
fn route_hop_gestures_prepare_only_bound_kind_and_keep_original_epoch_guards() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let password = password_identity(&f).await?;
        let snapshot = f
            .store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                pulse_store::ssh_hosts::HostEdit::Create(
                    pulse_core::ssh_endpoint::SshEndpoint::new(
                        "bastion",
                        "jump.invalid",
                        22,
                        "user",
                    )?,
                ),
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
        key_interactive::check(&f, via, leaf, password).await?;
        let route = f
            .store
            .load_ssh_route_source(
                leaf,
                SshRouteIdentity::PrivateKey(f.identity),
                Arc::new(|| true),
            )
            .await?;
        let outer = route.hop_source(0)?;
        let final_source = route.hop_source(1)?;
        interactive::check(&f, &route, password, leaf).await?;
        assert!(
            saved(&f, f.identity)?
                .select_source(&f.source().await?)
                .is_err()
        );
        assert!(saved(&f, password)?.select_source(&final_source).is_err());
        assert!(
            f.owner
                .begin_saved_session_gesture_authorized(
                    f.page.scope.clone(),
                    entrance,
                    f.identity,
                    Arc::new(|| true)
                )?
                .select_source(&outer)
                .is_err()
        );
        let final_gesture = match saved(&f, f.identity)?.select_source(&final_source)? {
            SavedSessionSelection::PrivateKey(g) => g,
            _ => return Err("fixture.kind".into()),
        };
        let outer_gesture = match f
            .owner
            .begin_saved_session_gesture_authorized(
                f.page.scope.clone(),
                entrance,
                password,
                Arc::new(|| true),
            )?
            .select_source(&outer)?
        {
            SavedSessionSelection::Password(g) => g,
            _ => return Err("fixture.kind".into()),
        };
        let outer_intent = outer_gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, outer.password_source().clone())
            .await?;
        assert!(outer_intent.authorization()());
        drop(outer_intent);
        let final_intent = final_gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, final_source)
            .await?;
        let guard = final_intent.authorization();
        assert!(guard());
        let scope = f
            .owner
            .inner
            .guard()?
            .scope
            .clone()
            .ok_or("fixture.scope")?;
        scope.lock()?;
        assert!(!guard());
        drop(final_intent);
        f.finish().await
    })
}
