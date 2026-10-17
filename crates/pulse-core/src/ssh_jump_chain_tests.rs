use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
type Components = (Vec<Host>, Vec<Endpoint>, Vec<SshProfile>);
fn id(n: u16) -> Uuid {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[14..].copy_from_slice(&n.to_be_bytes());
    Uuid::from_bytes(bytes)
}
fn meta(n: u16, deleted: bool) -> Result<RecordIdentity, InvalidCatalogue> {
    RecordIdentity::new(
        id(n),
        100,
        Hlc::new(101, 1, id(99)).map_err(|_| InvalidCatalogue)?,
        deleted,
    )
}
fn components(count: u16) -> Result<Components, InvalidCatalogue> {
    let host = Host::new(meta(1, false)?, "machine")?;
    let mut endpoints = Vec::new();
    let mut profiles = Vec::new();
    for index in 0..count {
        endpoints.push(Endpoint::new(
            meta(index + 2, false)?,
            id(1),
            "SSH",
            EndpointProtocol::Ssh,
            "fixture.invalid",
            22,
        )?);
        profiles.push(
            SshProfile::new(
                meta(index + 100, false)?,
                id(index + 2),
                "profile",
                "user",
                None,
            )?
            .with_jump_profile((index > 0).then(|| id(index + 99)))?,
        );
    }
    Ok((vec![host], endpoints, profiles))
}
#[test]
fn eight_jumps_resolve_in_order_and_ninth_jump_is_rejected() -> TestResult {
    let (hosts, endpoints, profiles) = components(9)?;
    let graph = HostCatalogue::new(hosts, endpoints, profiles)?;
    assert!(graph.ssh_jump_chain(id(100))?.is_empty());
    assert_eq!(
        graph
            .ssh_jump_chain(id(108))?
            .iter()
            .map(|p| p.metadata().id())
            .collect::<Vec<_>>(),
        (100..108).map(id).collect::<Vec<_>>()
    );
    let (hosts, endpoints, profiles) = components(10)?;
    assert!(HostCatalogue::new(hosts, endpoints, profiles).is_err());
    Ok(())
}
#[test]
fn cycles_foreign_kinds_missing_or_deleted_hops_never_resolve() -> TestResult {
    for invalid in [id(1), id(2), id(999)] {
        let (hosts, endpoints, mut profiles) = components(2)?;
        profiles[1] = profiles[1].clone().with_jump_profile(Some(invalid))?;
        assert!(HostCatalogue::new(hosts, endpoints, profiles).is_err());
    }
    let (hosts, endpoints, mut profiles) = components(2)?;
    assert!(
        profiles[0]
            .clone()
            .with_jump_profile(Some(id(100)))
            .is_err()
    );
    profiles[0] = profiles[0].clone().with_jump_profile(Some(id(101)))?;
    assert!(HostCatalogue::new(hosts, endpoints, profiles).is_err());
    let (hosts, endpoints, mut profiles) = components(2)?;
    profiles[0].metadata = meta(100, true)?;
    assert!(HostCatalogue::new(hosts.clone(), endpoints.clone(), profiles.clone()).is_err());
    profiles[1].metadata = meta(101, true)?;
    let graph = HostCatalogue::new(hosts, endpoints, profiles)?;
    assert!(graph.ssh_jump_chain(id(101)).is_err());
    Ok(())
}
