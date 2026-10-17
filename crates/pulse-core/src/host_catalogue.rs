//! Independently identified machines, protocol entrances and SSH preferences, without I/O.
use crate::{hlc::Hlc, ssh_target::SshTarget};
use std::collections::HashSet;
use uuid::Uuid;
/// Static failure; never includes caller metadata, targets or credentials.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("host.catalogue.invalid")]
pub struct InvalidCatalogue;
/// Synchronizable identity assigned by storage; identity and timestamps convey no permission.
#[derive(Clone, PartialEq, Eq)]
pub struct RecordIdentity {
    id: Uuid,
    created_at: u64,
    clock: Hlc,
    deleted: bool,
}
impl RecordIdentity {
    /// Validate UUIDv7 and positive creation time at or before the last-edit HLC.
    /// Clock generation, CAS and restoration policy belong to storage.
    pub fn new(
        id: Uuid,
        created_at: u64,
        clock: Hlc,
        deleted: bool,
    ) -> Result<Self, InvalidCatalogue> {
        if !valid_id(id) || created_at == 0 || created_at > clock.wall() {
            return Err(InvalidCatalogue);
        }
        Ok(Self {
            id,
            created_at,
            clock,
            deleted,
        })
    }
    /// Backend record identity, never a connection ticket.
    pub fn id(&self) -> Uuid {
        self.id
    }
    /// Backend creation milliseconds.
    pub fn created_at(&self) -> u64 {
        self.created_at
    }
    /// Last-edit milliseconds from the bounded HLC.
    pub fn updated_at(&self) -> u64 {
        self.clock.wall()
    }
    /// Ordering metadata, not authorization expiry or store affinity.
    pub fn clock(&self) -> Hlc {
        self.clock
    }
    /// Tombstones retain identity and cannot participate in active references.
    pub fn deleted(&self) -> bool {
        self.deleted
    }
}
fn valid_id(id: Uuid) -> bool {
    id.get_variant() == uuid::Variant::RFC4122 && id.get_version_num() == 7
}
fn label(value: &str) -> Result<String, InvalidCatalogue> {
    if !crate::ssh_metadata::valid(value, "metadata") {
        return Err(InvalidCatalogue);
    }
    Ok(value.to_owned())
}
/// Machine independent of protocol, login, identity and credential bytes.
#[derive(Clone, PartialEq, Eq)]
pub struct Host {
    metadata: RecordIdentity,
    label: String,
}
impl Host {
    /// Construct bounded literal display metadata, without an endpoint or network operation.
    pub fn new(metadata: RecordIdentity, name: &str) -> Result<Self, InvalidCatalogue> {
        Ok(Self {
            metadata,
            label: label(name)?,
        })
    }
    /// Storage and synchronization identity.
    pub fn metadata(&self) -> &RecordIdentity {
        &self.metadata
    }
    /// Display only; never a network target.
    pub fn label(&self) -> &str {
        &self.label
    }
}
/// Explicit protocol metadata. Presence here does not enable a transport.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EndpointProtocol {
    /// Secure shell terminal, separately authorized by the SSH adapter.
    Ssh,
    /// SSH file transfer; no terminal or file-use permission is implied.
    Sftp,
    /// Remote Desktop, reserved until the RDP adapter is available.
    Rdp,
    /// Virtual Network Computing, reserved until the VNC adapter is available.
    Vnc,
    /// Telnet, reserved until its adapter is available.
    Telnet,
}
impl EndpointProtocol {
    /// Suggested editor port, not an automatically selected dial destination.
    pub fn default_port(self) -> u16 {
        match self {
            Self::Ssh | Self::Sftp => 22,
            Self::Rdp => 3389,
            Self::Vnc => 5900,
            Self::Telnet => 23,
        }
    }
}
/// One independently identified protocol entrance owned by one machine.
#[derive(Clone, PartialEq, Eq)]
pub struct Endpoint {
    metadata: RecordIdentity,
    host_id: Uuid,
    label: String,
    protocol: EndpointProtocol,
    target: SshTarget,
}
impl Endpoint {
    /// Reuse the canonical DNS/IP/TCP parser for address metadata without usernames.
    /// Runtime transport support and authorization must be checked separately.
    pub fn new(
        metadata: RecordIdentity,
        host_id: Uuid,
        name: &str,
        protocol: EndpointProtocol,
        address: &str,
        port: u16,
    ) -> Result<Self, InvalidCatalogue> {
        if !valid_id(host_id) || host_id == metadata.id() {
            return Err(InvalidCatalogue);
        }
        Ok(Self {
            metadata,
            host_id,
            label: label(name)?,
            protocol,
            target: SshTarget::new(address, port).map_err(|_| InvalidCatalogue)?,
        })
    }
    /// Endpoint identity, separate from the parent machine.
    pub fn metadata(&self) -> &RecordIdentity {
        &self.metadata
    }
    /// Machine reference, checked against the full catalogue before saving.
    pub fn host_id(&self) -> Uuid {
        self.host_id
    }
    /// Literal endpoint display name.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Explicit transport; unsupported kinds must not fall back to SSH.
    pub fn protocol(&self) -> EndpointProtocol {
        self.protocol
    }
    /// Canonical destination metadata, not a dial permission.
    pub fn target(&self) -> &SshTarget {
        &self.target
    }
}
/// SSH preferences reference an entrance and reusable identity without copying a host.
#[derive(Clone, PartialEq, Eq)]
pub struct SshProfile {
    metadata: RecordIdentity,
    label: String,
    endpoint_id: Uuid,
    username: String,
    identity_id: Option<Uuid>,
    jump_profile_id: Option<Uuid>,
}
impl SshProfile {
    /// External identity existence, kind, epoch and target use are checked by the adapter.
    /// This record contains no credential, path or executable.
    pub fn new(
        metadata: RecordIdentity,
        endpoint_id: Uuid,
        name: &str,
        username: &str,
        identity_id: Option<Uuid>,
    ) -> Result<Self, InvalidCatalogue> {
        if !valid_id(endpoint_id)
            || endpoint_id == metadata.id()
            || !crate::ssh_metadata::valid(name, username)
            || identity_id
                .is_some_and(|id| !valid_id(id) || id == metadata.id() || id == endpoint_id)
        {
            return Err(InvalidCatalogue);
        }
        Ok(Self {
            metadata,
            endpoint_id,
            label: name.to_owned(),
            username: username.to_owned(),
            identity_id,
            jump_profile_id: None,
        })
    }
    /// Set a saved SSH Profile reference. The complete catalogue must validate the resulting chain.
    /// This configures metadata only; each hop still requires its own peer and identity approval.
    pub fn with_jump_profile(mut self, jump: Option<Uuid>) -> Result<Self, InvalidCatalogue> {
        if jump
            .is_some_and(|id| !valid_id(id) || id == self.metadata.id() || id == self.endpoint_id)
        {
            return Err(InvalidCatalogue);
        }
        self.jump_profile_id = jump;
        Ok(self)
    }
    /// Immediate saved jump Profile, never a machine, endpoint or dial permission.
    pub fn jump_profile_id(&self) -> Option<Uuid> {
        self.jump_profile_id
    }
    /// Profile identity and synchronization metadata.
    pub fn metadata(&self) -> &RecordIdentity {
        &self.metadata
    }
    /// Literal profile display name.
    pub fn label(&self) -> &str {
        &self.label
    }
    /// Explicit SSH entrance, never the parent machine ID.
    pub fn endpoint_id(&self) -> Uuid {
        self.endpoint_id
    }
    /// Login metadata, not a command argument or credential-use grant.
    pub fn username(&self) -> &str {
        &self.username
    }
    /// Saved identity reference; absence does not allow an identity fallback.
    pub fn identity_id(&self) -> Option<Uuid> {
        self.identity_id
    }
}
/// Bounded graph including tombstones. Storage must also verify external identity sources.
#[derive(Clone, PartialEq, Eq)]
pub struct HostCatalogue {
    hosts: Vec<Host>,
    endpoints: Vec<Endpoint>,
    profiles: Vec<SshProfile>,
}
impl HostCatalogue {
    /// Reject duplicate IDs, dangling/deleted owners and cross-protocol profile references.
    /// Tombstones count toward quotas; local entity IDs cannot impersonate external identities.
    pub fn new(
        hosts: Vec<Host>,
        endpoints: Vec<Endpoint>,
        profiles: Vec<SshProfile>,
    ) -> Result<Self, InvalidCatalogue> {
        if hosts.len() > 256 || endpoints.len() > 1024 || profiles.len() > 1024 {
            return Err(InvalidCatalogue);
        }
        let mut ids = HashSet::new();
        for meta in hosts
            .iter()
            .map(Host::metadata)
            .chain(endpoints.iter().map(Endpoint::metadata))
            .chain(profiles.iter().map(SshProfile::metadata))
        {
            if !ids.insert(meta.id()) {
                return Err(InvalidCatalogue);
            }
        }
        for endpoint in &endpoints {
            let host = hosts
                .iter()
                .find(|h| h.metadata.id() == endpoint.host_id)
                .ok_or(InvalidCatalogue)?;
            if !endpoint.metadata.deleted() && host.metadata.deleted() {
                return Err(InvalidCatalogue);
            }
        }
        for profile in &profiles {
            let endpoint = endpoints
                .iter()
                .find(|e| e.metadata.id() == profile.endpoint_id)
                .ok_or(InvalidCatalogue)?;
            if (!profile.metadata.deleted()
                && (endpoint.metadata.deleted() || endpoint.protocol != EndpointProtocol::Ssh))
                || profile.identity_id.is_some_and(|id| ids.contains(&id))
            {
                return Err(InvalidCatalogue);
            }
        }
        let catalogue = Self {
            hosts,
            endpoints,
            profiles,
        };
        for profile in catalogue.profiles.iter().filter(|p| !p.metadata.deleted()) {
            catalogue.ssh_jump_chain(profile.metadata.id())?;
        }
        Ok(catalogue)
    }
    /// Resolve at most eight saved jumps, ordered from the outermost hop toward the destination.
    /// The destination is omitted. A direct profile returns an empty chain; no I/O is performed.
    /// Tombstones retain historical references but cannot serve as a live hop or destination.
    pub fn ssh_jump_chain(&self, destination: Uuid) -> Result<Vec<&SshProfile>, InvalidCatalogue> {
        let mut current = self
            .profiles
            .iter()
            .find(|p| p.metadata.id() == destination && !p.metadata.deleted())
            .ok_or(InvalidCatalogue)?;
        let mut seen = HashSet::from([destination]);
        let mut chain = Vec::new();
        while let Some(id) = current.jump_profile_id {
            if chain.len() == 8 || !seen.insert(id) {
                return Err(InvalidCatalogue);
            }
            current = self
                .profiles
                .iter()
                .find(|p| p.metadata.id() == id && !p.metadata.deleted())
                .ok_or(InvalidCatalogue)?;
            chain.push(current);
        }
        chain.reverse();
        Ok(chain)
    }
    /// Host rows, including tombstones; membership implies no permission.
    pub fn hosts(&self) -> &[Host] {
        &self.hosts
    }
    /// Protocol entrances, including tombstones.
    pub fn endpoints(&self) -> &[Endpoint] {
        &self.endpoints
    }
    /// SSH preferences, including tombstones.
    pub fn profiles(&self) -> &[SshProfile] {
        &self.profiles
    }
}
#[cfg(test)]
#[path = "ssh_jump_chain_tests.rs"]
mod jump_tests;
#[cfg(test)]
mod tests {
    use super::*;
    fn id(n: u16) -> Uuid {
        let mut b = [0u8; 16];
        b[6] = 0x70;
        b[8] = 0x80;
        b[14..].copy_from_slice(&n.to_be_bytes());
        Uuid::from_bytes(b)
    }
    fn meta(n: u16, deleted: bool) -> Result<RecordIdentity, Box<dyn std::error::Error>> {
        Ok(RecordIdentity::new(
            id(n),
            100,
            Hlc::new(101, 1, id(99))?,
            deleted,
        )?)
    }
    fn entrance(
        n: u16,
        p: EndpointProtocol,
        deleted: bool,
    ) -> Result<Endpoint, Box<dyn std::error::Error>> {
        Ok(Endpoint::new(
            meta(n, deleted)?,
            id(1),
            "入口",
            p,
            "EXAMPLE.COM",
            p.default_port(),
        )?)
    }
    #[test]
    fn one_machine_multiple_protocols_and_independent_profile_references()
    -> Result<(), Box<dyn std::error::Error>> {
        let endpoints = [
            (2, EndpointProtocol::Ssh),
            (3, EndpointProtocol::Sftp),
            (4, EndpointProtocol::Rdp),
            (5, EndpointProtocol::Vnc),
            (6, EndpointProtocol::Telnet),
        ]
        .into_iter()
        .map(|(n, p)| entrance(n, p, false))
        .collect::<Result<Vec<_>, _>>()?;
        let profile = SshProfile::new(meta(7, false)?, id(2), "管理员", "用户", Some(id(8)))?;
        let catalogue = HostCatalogue::new(
            vec![Host::new(meta(1, false)?, "同一主机")?],
            endpoints,
            vec![profile],
        )?;
        assert_eq!(catalogue.hosts().len(), 1);
        assert_eq!(catalogue.endpoints().len(), 5);
        assert!(
            catalogue
                .endpoints()
                .iter()
                .all(|e| e.host_id() == id(1) && e.target().host() == "example.com")
        );
        assert_eq!(catalogue.endpoints()[2].target().port(), 3389);
        assert_eq!(catalogue.profiles()[0].endpoint_id(), id(2));
        assert_eq!(catalogue.profiles()[0].identity_id(), Some(id(8)));
        assert_eq!(catalogue.profiles()[0].metadata().updated_at(), 101);
        Ok(())
    }
    #[test]
    fn dangling_deleted_cross_kind_or_unsupported_profile_never_becomes_an_ssh_target()
    -> Result<(), Box<dyn std::error::Error>> {
        let host = Host::new(meta(1, false)?, "server")?;
        let endpoint = entrance(2, EndpointProtocol::Ssh, false)?;
        assert!(HostCatalogue::new(vec![], vec![endpoint.clone()], vec![]).is_err());
        assert!(
            HostCatalogue::new(
                vec![host.clone()],
                vec![endpoint.clone(), endpoint.clone()],
                vec![]
            )
            .is_err()
        );
        assert!(
            HostCatalogue::new(
                vec![Host::new(meta(1, true)?, "server")?],
                vec![endpoint.clone()],
                vec![]
            )
            .is_err()
        );
        let profile = SshProfile::new(meta(7, false)?, id(2), "ssh", "user", None)?;
        assert!(
            HostCatalogue::new(
                vec![host.clone()],
                vec![entrance(2, EndpointProtocol::Ssh, true)?],
                vec![profile.clone()]
            )
            .is_err()
        );
        assert!(
            HostCatalogue::new(
                vec![host.clone()],
                vec![entrance(2, EndpointProtocol::Rdp, false)?],
                vec![profile]
            )
            .is_err()
        );
        let wrong = SshProfile::new(meta(7, false)?, id(2), "ssh", "user", Some(id(1)))?;
        assert!(HostCatalogue::new(vec![host.clone()], vec![endpoint], vec![wrong]).is_err());
        assert!(
            Endpoint::new(
                meta(2, false)?,
                id(1),
                "unsafe",
                EndpointProtocol::Ssh,
                "user@host",
                22
            )
            .is_err()
        );
        assert!(SshProfile::new(meta(7, false)?, id(2), "ssh", "-option", None).is_err());
        assert!(RecordIdentity::new(Uuid::nil(), 100, Hlc::new(101, 0, id(99))?, false).is_err());
        assert!(RecordIdentity::new(id(2), 102, Hlc::new(101, 0, id(99))?, false).is_err());
        assert!(HostCatalogue::new(vec![host; 257], vec![], vec![]).is_err());
        Ok(())
    }
}
