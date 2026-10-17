use super::*;
use crate::ssh_agent_identities::{self as agents, AgentIdentityEdit};

#[test]
fn agent_route_source_three_kinds_exact_hops_and_same_revision_replacement() -> TestResult {
    let f = fixture()?;
    let saved = agents::edit(
        &f.db,
        f.owner,
        &agents::load(&f.db, f.owner)?,
        AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
            "route-agent",
            "route-user",
            "ssh-ed25519",
            &format!("SHA256:{}", "A".repeat(43)),
        )?),
        || true,
    )?;
    let id = saved.records().next().ok_or("fixture.agent")?.id();
    let profile = ssh_profiles::load(&f.db, f.owner)?;
    ssh_profiles::edit(
        &f.db,
        f.owner,
        &profile,
        ProfileMutation::Complete(
            ProfileEdit {
                id: f.profiles[1],
                label: "Agent hop".into(),
                username: "route-user".into(),
                identity: Some(id),
            },
            Some(f.profiles[0]),
        ),
        || true,
    )?;
    let source = load(
        &f.db,
        f.owner,
        f.profiles[2],
        SshRouteIdentity::PrivateKey(f.key),
    )?;
    assert!(source.hops()[0].identity() == SshRouteIdentity::Password(f.password));
    assert!(source.hops()[1].identity() == SshRouteIdentity::Agent(id));
    assert!(source.hops()[2].identity() == SshRouteIdentity::PrivateKey(f.key));
    let middle = source.hop_source(1)?;
    assert!(middle.supports_agent_selection(f.entrances[1], id));
    assert!(!middle.supports_agent_selection(f.entrances[2], id));
    assert!(!middle.supports_private_key_selection(f.entrances[1], id));
    assert!(
        !middle
            .password_source()
            .supports_password_selection(f.entrances[1], id)
    );
    assert!(
        middle.password_source().forward_target() == Some(source.hops()[2].endpoint().target())
    );
    let plain = source.target_source();
    assert!(!plain.supports_agent_selection(f.entrances[1], id));
    let agent_leaf = load(&f.db, f.owner, f.profiles[2], SshRouteIdentity::Agent(id))?;
    assert!(
        agent_leaf
            .hop_source(2)?
            .supports_agent_selection(f.entrances[2], id)
    );
    assert!(agent_leaf.hop_source(2)?.password_source().is_route_leaf());
    assert!(matches!(
        agents::edit(
            &f.db,
            f.owner,
            &saved,
            AgentIdentityEdit::Remove(id),
            || true
        ),
        Err(StoreError::IdentityConflict)
    ));
    let revision = saved.revision();
    f.db.execute(
        "UPDATE agent_identities SET label='replaced same revision' WHERE id=?1",
        [id.to_string()],
    )?;
    assert_eq!(agents::load(&f.db, f.owner)?.revision(), revision);
    assert!(!crate::ssh_target_source::same(
        middle.password_source(),
        crate::ssh_route_source::load(
            &f.db,
            f.owner,
            f.profiles[2],
            SshRouteIdentity::PrivateKey(f.key)
        )?
        .hop_source(1)?
        .password_source()
    )?);
    // A persisted collision must never be inferred as either of its kinds.
    f.db.execute(
        "UPDATE agent_identities SET id=?1 WHERE id=?2",
        [f.password.to_string(), id.to_string()],
    )?;
    assert!(
        load(
            &f.db,
            f.owner,
            f.profiles[2],
            SshRouteIdentity::Password(f.password)
        )
        .is_err()
    );
    assert!(
        load(
            &f.db,
            f.owner,
            f.profiles[2],
            SshRouteIdentity::Agent(f.password)
        )
        .is_err()
    );
    Ok(())
}
