//! Joint authentication must retain the exact intermediate/leaf purpose before touching a sink.
use super::*;
use crate::vault_owner::{
    AuthenticatedProtocolParts, PrivateKeyForwardSink, PrivateKeyInteractiveForwardSink,
    PrivateKeyInteractiveSessionSink, PrivateKeySessionSink, PrivateKeySourceConfirmation,
    PrivateKeyTargetSink,
};
use pulse_store::ssh_key_target_source::SshKeyTargetSource;
use std::{future::Future, pin::Pin};
type R<T> = std::result::Result<T, VaultOwnerError>;
#[derive(Default)]
struct Unused {
    calls: usize,
}
impl PrivateKeyTargetSink for Unused {
    fn matches_target(&self, _: &SshKeyTargetSource, _: Uuid, _: Uuid) -> bool {
        true
    }
    fn retain_native_authorization(&mut self, _: Authorize) -> R<()> {
        self.calls += 1;
        Ok(())
    }
    fn authenticate<'a>(
        &'a mut self,
        _: &'a str,
        _: &'a [u8],
        _: Option<&'a [u8]>,
        _: &'a PrivateKeySourceConfirmation,
    ) -> Pin<Box<dyn Future<Output = R<bool>> + Send + 'a>> {
        self.calls += 1;
        Box::pin(async { Ok(true) })
    }
    fn close_and_drain(&mut self) -> Pin<Box<dyn Future<Output = R<()>> + Send + '_>> {
        self.calls += 1;
        Box::pin(async { Ok(()) })
    }
}
impl PrivateKeySessionSink for Unused {
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
impl PrivateKeyForwardSink for Unused {
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
impl PrivateKeyInteractiveSessionSink for Unused {}
impl PrivateKeyInteractiveForwardSink for Unused {}
pub(super) async fn check(f: &Fixture, via: Uuid, leaf: Uuid, password: Uuid) -> TestResult {
    let original = f
        .store
        .load_ssh_route_source(
            leaf,
            SshRouteIdentity::PrivateKey(f.identity),
            Arc::new(|| true),
        )
        .await?;
    let mut account = saved(f, f.identity)?.select_route(original)?;
    assert!(
        account
            .take_next_selection()?
            .into_key_interactive()
            .is_err(),
        "an account capture cannot authorize private-key MFA"
    );
    drop(account);
    f.store
        .edit_ssh_profile(
            f.store
                .load_ssh_profiles_authorized(Arc::new(|| true))
                .await?,
            ProfileEdit {
                id: via,
                label: "bastion".into(),
                username: "user".into(),
                identity: Some(f.identity),
            },
            Arc::new(|| true),
        )
        .await?;
    let route = f
        .store
        .load_ssh_route_source(
            leaf,
            SshRouteIdentity::PrivateKey(f.identity),
            Arc::new(|| true),
        )
        .await?;
    let mut sink = Unused::default();
    for index in 0..2 {
        let mut parent = saved(f, f.identity)?.select_route(route.clone())?;
        for _ in 0..index {
            drop(parent.take_next_selection()?);
        }
        let (gesture, source) = parent
            .take_next_selection()?
            .into_key_interactive()?
            .into_parts();
        let RouteHopGesture::PrivateKeyInteractive(gesture) = gesture else {
            return Err("fixture.joint_kind".into());
        };
        let intent = gesture
            .begin_session_authorized(&f.owner, &f.page.scope, Arc::new(|| true))?
            .prepare_route(&f.store, source)
            .await?;
        if index == 0 {
            let mut started = None;
            assert!(
                intent
                    .authenticate_route_terminal(&f.store, &mut sink, &mut started)
                    .await
                    .is_err()
            );
            assert!(
                started.is_none(),
                "intermediate hop cannot allocate a terminal"
            );
        } else {
            assert!(
                intent
                    .authenticate_forward(&f.store, &mut sink)
                    .await
                    .is_err(),
                "leaf cannot open a forward"
            );
        }
        assert_eq!(
            sink.calls, 0,
            "role rejection must precede credential borrowing and native I/O"
        );
        let guard = intent.authorization();
        assert!(guard());
        intent.cancellation().cancel();
        assert!(
            !guard(),
            "local cancellation retains the original authority boundary"
        );
        drop(intent);
    }
    f.store
        .edit_ssh_profile(
            f.store
                .load_ssh_profiles_authorized(Arc::new(|| true))
                .await?,
            ProfileEdit {
                id: via,
                label: "bastion".into(),
                username: "user".into(),
                identity: Some(password),
            },
            Arc::new(|| true),
        )
        .await?;
    Ok(())
}
