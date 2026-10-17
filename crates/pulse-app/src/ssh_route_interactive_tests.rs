//! Roles remain distinct even after explicitly choosing interactive authentication.
use super::*;
use crate::vault_owner::{
    AuthenticatedProtocolParts, InteractiveForwardSink, InteractiveSessionSink,
};
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use std::{future::Future, pin::Pin};
type R<T> = std::result::Result<T, VaultOwnerError>;
#[derive(Default)]
struct Unused {
    calls: usize,
}
impl InteractiveSessionSink for Unused {
    fn matches_target(&self, _: &SshKeyTargetSource, _: Uuid, _: Uuid) -> bool {
        true
    }
    fn retain_native_authorization(&mut self, _: Authorize) -> R<()> {
        self.calls += 1;
        Ok(())
    }
    fn authenticate_interactive<'a>(
        &'a mut self,
        _: &'a str,
        _: Authorize,
    ) -> Pin<Box<dyn Future<Output = R<bool>> + Send + 'a>> {
        self.calls += 1;
        Box::pin(async { Ok(true) })
    }
    fn prepare_authenticated_terminal(
        &mut self,
        _: Authorize,
    ) -> Pin<Box<dyn Future<Output = R<()>> + Send + '_>> {
        self.calls += 1;
        Box::pin(async { Ok(()) })
    }
    fn take_authenticated_root_parts(&mut self) -> R<AuthenticatedProtocolParts> {
        self.calls += 1;
        Err(VaultOwnerError::Denied)
    }
}
impl InteractiveForwardSink for Unused {
    type Transport = ();
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        _: &'a pulse_core::ssh_target::SshTarget,
        _: Authorize,
    ) -> Pin<Box<dyn Future<Output = R<()>> + Send + 'a>> {
        self.calls += 1;
        Box::pin(async { Ok(()) })
    }
    fn take_authenticated_forward(&mut self) -> R<()> {
        self.calls += 1;
        Ok(())
    }
}
pub(super) async fn check(
    f: &Fixture,
    route: &pulse_store::ssh_route_source::SshRouteSource,
    password: Uuid,
    leaf: Uuid,
) -> TestResult {
    let mut parent = f
        .owner
        .begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            f.identity,
            Arc::new(|| true),
        )?
        .select_route(route.clone())?;
    let (owner, outer) = parent
        .take_next_selection()?
        .into_interactive()?
        .into_parts();
    let RouteHopGesture::Interactive(owner) = owner else {
        return Err("fixture.interactive_kind".into());
    };
    let intent = owner
        .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
        .prepare_route(&f.store, outer)
        .await?;
    let mut sink = Unused::default();
    let mut started = None;
    assert!(
        intent
            .authenticate_terminal(&f.store, &mut sink, &mut started)
            .await
            .is_err()
    );
    assert!(started.is_none());
    assert_eq!(
        sink.calls, 0,
        "a forward-only hop cannot authenticate or allocate a terminal"
    );
    assert!(parent.take_next_selection()?.into_interactive().is_err());
    drop(intent);
    let password_route = f
        .store
        .load_ssh_route_source(
            leaf,
            SshRouteIdentity::Password(password),
            Arc::new(|| true),
        )
        .await?;
    let mut parent = f
        .owner
        .begin_saved_session_gesture_authorized(
            f.page.scope.clone(),
            f.host,
            password,
            Arc::new(|| true),
        )?
        .select_route(password_route)?;
    drop(parent.take_next_selection()?);
    let (owner, terminal) = parent
        .take_next_selection()?
        .into_interactive()?
        .into_parts();
    let RouteHopGesture::Interactive(owner) = owner else {
        return Err("fixture.interactive_kind".into());
    };
    let intent = owner
        .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
        .prepare_route(&f.store, terminal)
        .await?;
    assert!(
        intent
            .authenticate_forward(&f.store, &mut sink)
            .await
            .is_err()
    );
    assert_eq!(sink.calls, 0, "a leaf cannot allocate a forward stream");
    Ok(())
}
