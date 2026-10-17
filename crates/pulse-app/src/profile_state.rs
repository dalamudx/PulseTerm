use pulse_pty::profiles::{AutoProfileCatalog, ProfileError, ShellCandidate, ShellKind};
use pulse_store::{
    StoreError,
    profiles::{AutoProfilePreferences, AutomaticShell},
};
fn to_kind(shell: AutomaticShell) -> ShellKind {
    match shell {
        AutomaticShell::Bash => ShellKind::Bash,
        AutomaticShell::Zsh => ShellKind::Zsh,
        AutomaticShell::Fish => ShellKind::Fish,
        AutomaticShell::Sh => ShellKind::Sh,
        AutomaticShell::Pwsh => ShellKind::Pwsh,
    }
}
fn to_shell(kind: ShellKind) -> AutomaticShell {
    match kind {
        ShellKind::Bash => AutomaticShell::Bash,
        ShellKind::Zsh => AutomaticShell::Zsh,
        ShellKind::Fish => AutomaticShell::Fish,
        ShellKind::Sh => AutomaticShell::Sh,
        ShellKind::Pwsh => AutomaticShell::Pwsh,
    }
}
/// Rebuild from validated local metadata and trusted fresh discovery only.
/// Stored preferences never provide an executable or authorize a session launch.
pub fn restore_catalog(
    preferences: &AutoProfilePreferences,
    candidates: &[ShellCandidate],
) -> Result<AutoProfileCatalog, ProfileError> {
    let mut catalog = AutoProfileCatalog::from_preferences(
        preferences.hidden().map(to_kind),
        preferences.deleted().map(to_kind),
    )?;
    catalog.reconcile(candidates)?;
    Ok(catalog)
}
/// Resolve an explicit saved default against a fresh, trusted discovery snapshot.
/// Missing, hidden or deleted choices fail closed; no other shell is selected.
/// The result is metadata only and still requires normal current-page launch admission.
pub fn resolve_default_profile(
    snapshot: &pulse_store::profile_default::DefaultProfileSnapshot,
    candidates: &[ShellCandidate],
) -> Result<Option<ShellKind>, ProfileError> {
    let catalog = restore_catalog(snapshot.preferences(), candidates)?;
    let Some(shell) = snapshot.shell() else {
        return Ok(None);
    };
    let kind = to_kind(shell);
    let profile = catalog.profile(kind).ok_or(ProfileError::Unavailable)?;
    if profile.is_hidden() || !profile.is_available() {
        return Err(ProfileError::Unavailable);
    }
    Ok(Some(kind))
}
/// Snapshot non-secret preferences for a typed store write. Persist successfully
/// before replacing live state; no automatic write retry or UI mutation is implied.
pub fn catalog_preferences(
    catalog: &AutoProfileCatalog,
) -> Result<AutoProfilePreferences, StoreError> {
    let mut preferences = AutoProfilePreferences::default();
    for kind in catalog.deleted() {
        preferences.suppress(to_shell(kind));
    }
    for kind in catalog.hidden() {
        preferences.set_hidden(to_shell(kind), true)?;
    }
    Ok(preferences)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn candidate(kind: ShellKind) -> Result<ShellCandidate, ProfileError> {
        ShellCandidate::new(
            kind,
            std::env::current_dir()
                .map_err(|_| ProfileError::InvalidExecutable)?
                .join(kind.executable_name()),
        )
    }
    #[test]
    fn missing_hidden_profile_survives_until_discovered_and_deleted_stays_suppressed()
    -> Result<(), Box<dyn std::error::Error>> {
        let mut p = AutoProfilePreferences::default();
        p.set_hidden(AutomaticShell::Bash, true)?;
        p.suppress(AutomaticShell::Zsh);
        let mut c = restore_catalog(&p, &[])?;
        assert_eq!(catalog_preferences(&c)?, p);
        c.reconcile(&[candidate(ShellKind::Bash)?, candidate(ShellKind::Zsh)?])?;
        assert!(
            c.profile(ShellKind::Bash)
                .ok_or("missing Bash")?
                .is_hidden()
        );
        assert!(c.profile(ShellKind::Zsh).is_none());
        assert_eq!(catalog_preferences(&c)?, p);
        c.set_hidden(ShellKind::Bash, false)?;
        assert!(catalog_preferences(&c)?.hidden().next().is_none());
        c.set_hidden(ShellKind::Bash, true)?;
        c.remove(ShellKind::Bash)?;
        assert!(c.hidden().next().is_none());
        assert_eq!(catalog_preferences(&c)?.deleted().count(), 2);
        Ok(())
    }
    #[test]
    fn five_identities_roundtrip_and_conflicts_are_rejected()
    -> Result<(), Box<dyn std::error::Error>> {
        for kind in [
            ShellKind::Bash,
            ShellKind::Zsh,
            ShellKind::Fish,
            ShellKind::Sh,
            ShellKind::Pwsh,
        ] {
            let c = AutoProfileCatalog::from_preferences([kind], [])?;
            let p = catalog_preferences(&c)?;
            let r = restore_catalog(&p, &[candidate(kind)?])?;
            assert_eq!(catalog_preferences(&r)?, p);
            assert!(r.profile(kind).ok_or("missing profile")?.is_hidden());
        }
        assert!(matches!(
            AutoProfileCatalog::from_preferences([ShellKind::Bash], [ShellKind::Bash]),
            Err(ProfileError::ConflictingPreferences)
        ));
        assert!(matches!(
            AutoProfileCatalog::from_preferences([ShellKind::Bash, ShellKind::Bash], []),
            Err(ProfileError::ConflictingPreferences)
        ));
        assert!(matches!(
            AutoProfileCatalog::from_preferences(std::iter::repeat(ShellKind::Bash), []),
            Err(ProfileError::DiscoveryLimit)
        ));
        Ok(())
    }
}
