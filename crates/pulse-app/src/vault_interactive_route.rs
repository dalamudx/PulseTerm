//! Intermediate MFA can forward only to the next target frozen in this exact route source.
use super::*;
use pulse_core::ssh_target::SshTarget;

/// Trusted adapter; retain native capacity and this intent until failed protocol work is drained.
pub trait InteractiveForwardSink: InteractiveSessionSink {
    /// Owned stream and protocol completion evidence for the next frozen hop.
    type Transport: Send + 'static;
    /// Open only this already-authorized exact next-hop target.
    fn prepare_authenticated_forward<'a>(
        &'a mut self,
        target: &'a SshTarget,
        authorized: Authorize,
    ) -> Pin<Box<dyn Future<Output = Result<()>> + Send + 'a>>;
    /// Transfer the stream only after native protocol confirmation.
    fn take_authenticated_forward(&mut self) -> Result<Self::Transport>;
}
impl InteractiveSessionIntent {
    /// No terminal role, password read or page-supplied destination.
    pub async fn authenticate_forward<S: InteractiveForwardSink>(
        &self,
        store: &StateStore,
        sink: &mut S,
    ) -> Result<S::Transport> {
        let target = self
            .source
            .password_source()
            .forward_target()
            .ok_or(VaultOwnerError::Denied)?;
        let guard = self.authorization();
        let cancel = self.cancellation();
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.retain_native_authorization(guard.clone())?;
        let user = self
            .source
            .password_source()
            .identity_vault()
            .identities()
            .records()
            .find(|row| row.id() == self.selected.1)
            .ok_or(VaultOwnerError::Denied)?
            .identity()
            .username()
            .to_owned();
        let accepted = tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.authenticate_interactive(&user, guard.clone()) => result?,
        };
        if !accepted {
            return Err(VaultOwnerError::Denied);
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        tokio::select! { biased;
            _ = cancel.wait_cancelled() => return Err(VaultOwnerError::Denied),
            _ = tokio::time::sleep_until(tokio::time::Instant::from_std(self.deadline)) => return Err(VaultOwnerError::Denied),
            result = sink.prepare_authenticated_forward(target, guard.clone()) => result?,
        }
        store
            .validate_ssh_key_target_source(self.source.clone(), guard.clone())
            .await
            .map_err(|_| VaultOwnerError::Denied)?;
        if !guard() || !sink.matches_target(&self.source, self.selected.0, self.selected.1) {
            return Err(VaultOwnerError::Denied);
        }
        sink.take_authenticated_forward()
    }
}
