//! Frozen-hop role boundaries before any password loan or network work.
use super::*;
use crate::vault_owner::{PasswordForwardSink, PasswordInteractiveForwardSink, RouteHopGesture};
use pulse_store::{ssh_profiles::ProfileEdit, ssh_route_source::SshRouteIdentity};

impl PasswordInteractiveForwardSink for NoPasswordUse {}
impl PasswordForwardSink for NoPasswordUse {
    type Transport = ();
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        _: &'a pulse_core::ssh_target::SshTarget,
        _: crate::vault_owner::Authorize,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<()>> + Send + 'a>> {
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn take_authenticated_forward(&mut self) -> Result<()> {
        Err(VaultOwnerError::Denied)
    }
}

#[test]
fn route_password_joint_keeps_frozen_roles_and_direct_entry_separation() -> TestResult {
    let _serial = crate::VAULT_CRYPTO_TEST_LOCK
        .lock()
        .map_err(|_| "fixture.poison")?;
    runtime()?.block_on(async {
        let f = Fixture::new().await?;
        let identity = password_identity(&f).await?;
        f.seal(f.key, SecretKind::SshPrivateKey).await?;
        let approved = || {
            joint(&f, identity)?.begin_session_authorized(
                &f.owner,
                &f.page.scope,
                Arc::new(|| true),
            )
        };
        assert!(
            approved()?
                .prepare_route(
                    &f.store,
                    f.store.load_ssh_target_source(Arc::new(|| true)).await?
                )
                .await
                .is_err()
        );
        let hosts = f
            .store
            .edit_ssh_hosts(
                f.source().await?.hosts().clone(),
                pulse_store::ssh_hosts::HostEdit::Create(
                    pulse_core::ssh_endpoint::SshEndpoint::new(
                        "Password bastion",
                        "password-jump.invalid",
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
            .find(|row| row.id() != f.host)
            .ok_or("fixture.outer")?
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
        let bastion = profiles
            .profiles()
            .find(|p| p.endpoint_id() == outer)
            .ok_or("fixture.bastion")?
            .metadata()
            .id();
        let profiles = f
            .store
            .edit_ssh_profile(
                profiles,
                ProfileEdit {
                    id: bastion,
                    label: "Password bastion".into(),
                    username: "user".into(),
                    identity: Some(identity),
                },
                Arc::new(|| true),
            )
            .await?;
        f.store
            .edit_ssh_profile_jump(profiles, leaf, Some(bastion), Arc::new(|| true))
            .await?;
        let source = f
            .store
            .load_ssh_route_source(
                leaf,
                SshRouteIdentity::Password(identity),
                Arc::new(|| true),
            )
            .await?;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        let mut route = f
            .owner
            .begin_routed_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                identity,
                Arc::new(|| true),
            )?
            .tighten_deadline(deadline)
            .select_route(source)?;
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let mut sink = NoPasswordUse(calls.clone());
        for is_leaf in [false, true] {
            let (capture, hop) = route.take_next_selection()?.into_parts();
            let RouteHopGesture::Password(capture) = capture else {
                return Err("fixture.password_kind".into());
            };
            let source = hop.password_source().clone();
            assert_eq!(source.is_route_leaf(), is_leaf);
            assert!(
                PasswordInteractiveSessionGesture::from_saved_capture(capture)?
                    .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
                    .prepare(&f.store, source)
                    .await
                    .is_err()
            );
        }
        drop(route);
        // A second route capture tests accepted preparation and opposite-role zero use.
        let source = f
            .store
            .load_ssh_route_source(
                leaf,
                SshRouteIdentity::Password(identity),
                Arc::new(|| true),
            )
            .await?;
        let mut route = f
            .owner
            .begin_routed_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                identity,
                Arc::new(|| true),
            )?
            .tighten_deadline(deadline)
            .select_route(source)?;
        for is_leaf in [false, true] {
            let (capture, hop) = route
                .take_next_selection()?
                .into_password_interactive()?
                .into_parts();
            let RouteHopGesture::PasswordInteractive(capture) = capture else {
                return Err("fixture.password_kind".into());
            };
            let source = hop.password_source().clone();
            let intent = capture
                .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
                .prepare_route(&f.store, source)
                .await?;
            assert_eq!(intent.deadline(), deadline);
            let guard = intent.authorization();
            assert!(guard());
            if is_leaf {
                assert!(
                    intent
                        .authenticate_forward(&f.store, &mut sink)
                        .await
                        .is_err()
                );
            } else {
                let mut started = Some(
                    crate::protocol_session_owner::ProtocolSessionReservation::reserve(
                        &f.page.registry,
                        &f.page.scope,
                    )?
                    .begin()?,
                );
                assert!(
                    intent
                        .authenticate_route_terminal(&f.store, &mut sink, &mut started)
                        .await
                        .is_err()
                );
                started
                    .take()
                    .ok_or("fixture.reservation")?
                    .finish_unregistered(async { Ok(()) })
                    .join()
                    .await?;
            }
            assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 0);
            intent.cancellation().cancel();
            assert!(!guard());
            drop(guard);
            drop(intent);
        }
        drop(route);
        f.finish().await
    })
}
