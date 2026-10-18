//! Route roles and metadata conversions must reject before Agent signing or native allocation.
use super::*;
use crate::vault_owner::{AgentForwardSink, AgentInteractiveForwardSink};
use pulse_core::ssh_target::SshTarget;
use pulse_store::ssh_route_source::SshRouteSource;
use std::{future::Future, pin::Pin};

impl AgentInteractiveForwardSink for RejectUnexpectedSign {}
impl AgentForwardSink for RejectUnexpectedSign {
    type Transport = ();
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        _: &'a SshTarget,
        _: Arc<dyn Fn() -> bool + Send + Sync>,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), VaultOwnerError>> + Send + 'a>> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Box::pin(async { Err(VaultOwnerError::Denied) })
    }
    fn take_authenticated_forward(&mut self) -> std::result::Result<(), VaultOwnerError> {
        self.calls.fetch_add(1, Ordering::AcqRel);
        Err(VaultOwnerError::Denied)
    }
}

pub(super) async fn check(f: &Fixture, agent: Uuid, source: &SshRouteSource) -> TestResult {
    let capture = || {
        f.owner
            .begin_routed_session_gesture_authorized(
                f.page.scope.clone(),
                f.host,
                agent,
                Arc::new(|| true),
            )?
            .select_route(source.clone())
    };
    assert!(
        capture()?
            .take_next_selection()?
            .into_key_interactive()
            .is_err()
    );
    assert!(
        capture()?
            .take_next_selection()?
            .into_interactive()
            .is_err()
    );
    let mut wrong_purpose = capture()?;
    let (gesture, hop) = wrong_purpose
        .take_next_selection()?
        .into_agent_interactive()?
        .into_parts();
    let RouteHopGesture::AgentInteractive(gesture) = gesture else {
        return Err("fixture.joint-kind".into());
    };
    assert!(
        gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare(&f.store, hop)
            .await
            .is_err(),
        "a frozen route cannot use direct preparation"
    );
    drop(wrong_purpose);

    let calls = Arc::new(AtomicUsize::new(0));
    let mut sink = RejectUnexpectedSign {
        calls: calls.clone(),
    };
    let mut route = capture()?;
    let mut original_deadline = None;
    for index in 0..2 {
        let (gesture, hop) = route
            .take_next_selection()?
            .into_agent_interactive()?
            .into_parts();
        let RouteHopGesture::AgentInteractive(gesture) = gesture else {
            return Err("fixture.joint-kind".into());
        };
        let intent = gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare_route(&f.store, hop)
            .await?;
        if let Some(deadline) = original_deadline {
            assert_eq!(
                intent.deadline(),
                deadline,
                "a later hop cannot renew the original deadline"
            );
        } else {
            original_deadline = Some(intent.deadline());
        }
        if index == 0 {
            let mut started = Some(
                ProtocolSessionReservation::reserve(&f.page.registry, &f.page.scope)?.begin()?,
            );
            assert!(
                intent
                    .authenticate_route_terminal(&f.store, &mut sink, &mut started)
                    .await
                    .is_err()
            );
            started
                .take()
                .ok_or("fixture.start")?
                .finish_unregistered(async { Ok(()) })
                .join()
                .await?;
        } else {
            assert!(
                intent
                    .authenticate_forward(&f.store, &mut sink)
                    .await
                    .is_err()
            );
        }
        assert_eq!(
            calls.load(Ordering::Acquire),
            0,
            "role rejection must precede any Agent signing or native handoff"
        );
        let guard = intent.authorization();
        assert!(guard());
        intent.cancellation().cancel();
        assert!(!guard());
        drop(intent);
        assert!(
            matches!(
                f.owner.begin(f.page.scope.clone()),
                Err(VaultOwnerError::Busy)
            ),
            "a retained guard must still hold native capacity"
        );
        drop(guard);
    }
    drop(route);
    let mut stale = capture()?;
    let (gesture, _) = stale
        .take_next_selection()?
        .into_agent_interactive()?
        .into_parts();
    let RouteHopGesture::AgentInteractive(gesture) = gesture else {
        return Err("fixture.joint-kind".into());
    };
    restore_same_dek(f).await?;
    assert!(stale.take_next_selection().is_err());
    assert!(
        gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))
            .is_err()
    );
    Ok(())
}
