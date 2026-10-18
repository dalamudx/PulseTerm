//! Two exact-target forwarding peers for the marked private WebView fixture only.
use anyhow::{Context, Result, ensure};
use pulse_core::ssh_endpoint::SshEndpoint;
use pulse_store::{StateStore, ssh_hosts::HostEdit, ssh_profiles::ProfileEdit};
use russh::server::Server as _;
use russh::{Channel, server};
use std::{net::SocketAddr, sync::Arc};
#[path = "ssh_workspace_route_interactive_fixture.rs"]
mod interactive;
use tokio::net::{TcpListener, TcpStream};
use uuid::Uuid;
struct Jump {
    target: SocketAddr,
    label: &'static str,
    connection: bool,
    agent_key: Option<russh::keys::PublicKey>,
    interactive_round: Option<u8>,
    key_joint: bool,
    key_factor_complete: bool,
    password_joint: bool,
    password_factor_complete: bool,
}
impl Drop for Jump {
    fn drop(&mut self) {
        if self.connection {
            eprintln!("fixture.route.{}.connection_exited", self.label);
        }
    }
}
impl server::Server for Jump {
    type Handler = Self;
    fn new_client(&mut self, _: Option<SocketAddr>) -> Self {
        eprintln!("fixture.route.{}.connection_opened", self.label);
        Self {
            target: self.target,
            label: self.label,
            connection: true,
            agent_key: self.agent_key.clone(),
            interactive_round: self.interactive_round.map(|_| 0),
            key_joint: self.key_joint,
            key_factor_complete: false,
            password_joint: self.password_joint,
            password_factor_complete: false,
        }
    }
}
impl server::Handler for Jump {
    type Error = russh::Error;
    async fn auth_keyboard_interactive(
        &mut self,
        user: &str,
        _: &str,
        response: Option<server::Response<'_>>,
    ) -> Result<server::Auth, Self::Error> {
        if (self.key_joint && !self.key_factor_complete)
            || (self.password_joint && !self.password_factor_complete)
        {
            eprintln!("fixture.route.{}.premature_mfa_rejected", self.label);
            return Ok(server::Auth::reject());
        }
        interactive::answer(self, user, response)
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if self.password_joint {
            eprintln!("fixture.route.{}.password_attempt", self.label);
            if self.agent_key.is_some() || user != "user" || password != "fixed-target-secret" {
                return Ok(server::Auth::reject());
            }
            let policy = std::env::var_os("PULSE_SSH_PASSWORD_MFA_POLICY")
                .map(std::path::PathBuf::from)
                .and_then(|path| std::fs::read_to_string(path.with_extension(self.label)).ok());
            if policy.as_deref() == Some("partial") {
                self.password_factor_complete = true;
                eprintln!("fixture.route.{}.password_partial_success", self.label);
                return Ok(server::Auth::Reject {
                    proceed_with_methods: Some(
                        (&[russh::MethodKind::KeyboardInteractive][..]).into(),
                    ),
                    partial_success: true,
                });
            }
            if policy.as_deref() == Some("complete") {
                eprintln!("fixture.route.{}.password_complete_success", self.label);
                return Ok(server::Auth::Accept);
            }
            return Ok(server::Auth::reject());
        }
        Ok(
            if self.agent_key.is_none() && user == "user" && password == "fixed-target-secret" {
                eprintln!("fixture.route.{}.authenticated", self.label);
                server::Auth::Accept
            } else {
                server::Auth::reject()
            },
        )
    }
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if user == "user" && self.agent_key.as_ref() == Some(key) {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        Ok(if user == "user" && self.agent_key.as_ref() == Some(key) {
            if self.key_joint {
                let policy = std::env::var_os("PULSE_SSH_KEY_MFA_POLICY")
                    .map(std::path::PathBuf::from)
                    .and_then(|path| std::fs::read_to_string(path.with_extension(self.label)).ok());
                match policy.as_deref() {
                    Some("partial") => {
                        self.key_factor_complete = true;
                        eprintln!("fixture.route.{}.key_partial_success", self.label);
                        return Ok(server::Auth::Reject {
                            proceed_with_methods: Some(
                                (&[russh::MethodKind::KeyboardInteractive][..]).into(),
                            ),
                            partial_success: true,
                        });
                    }
                    Some("complete") => {
                        eprintln!("fixture.route.{}.key_complete_success", self.label);
                        eprintln!("fixture.route.{}.authenticated", self.label);
                        return Ok(server::Auth::Accept);
                    }
                    _ => {
                        eprintln!("fixture.route.{}.key_rejected", self.label);
                        return Ok(server::Auth::reject());
                    }
                }
            }
            eprintln!("fixture.route.{}.authenticated", self.label);
            eprintln!("fixture.route.{}.agent_authenticated", self.label);
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
    #[allow(clippy::too_many_arguments)]
    async fn channel_open_direct_tcpip(
        &mut self,
        channel: Channel<server::Msg>,
        host: &str,
        port: u32,
        _origin: &str,
        _origin_port: u32,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if host != "127.0.0.1" || port != u32::from(self.target.port()) {
            reply
                .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
                .await;
            return Ok(());
        }
        let mut socket = TcpStream::connect(self.target).await?;
        reply.accept().await;
        let label = self.label;
        eprintln!("fixture.route.{label}.forwarded");
        tokio::spawn(async move {
            let mut stream = channel.into_stream();
            let _ = tokio::io::copy_bidirectional(&mut stream, &mut socket).await;
            eprintln!("fixture.route.{label}.forward_exited");
        });
        Ok(())
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        eprintln!("fixture.route.{}.terminal_requested", self.label);
        reply
            .reject(russh::ChannelOpenFailure::AdministrativelyProhibited)
            .await;
        Ok(())
    }
}
pub(super) struct Servers {
    tasks: Vec<tokio::task::JoinHandle<()>>,
}
impl Drop for Servers {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}
pub(super) async fn seed(
    store: &StateStore,
    leaf: &TcpListener,
    identity: Uuid,
    agent_route: bool,
    interactive_route: bool,
) -> Result<Servers> {
    let key_interactive_route = std::env::args().nth(3).as_deref() == Some("key-interactive-route");
    let leaf_address = leaf.local_addr()?;
    let middle = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let middle_address = middle.local_addr()?;
    let outer = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let outer_address = outer.local_addr()?;
    let mut hosts = store.load_ssh_hosts().await?;
    for (label, address) in [
        ("GUI_JUMP_MIDDLE", middle_address),
        ("GUI_JUMP_OUTER", outer_address),
    ] {
        hosts = store
            .edit_ssh_hosts(
                hosts,
                HostEdit::Create(SshEndpoint::new(
                    label,
                    "127.0.0.1",
                    address.port(),
                    "user",
                )?),
                Arc::new(|| true),
            )
            .await?;
    }
    let graph = hosts.catalogue().context("fixture.route.graph")?;
    let profile_for = |label: &str| -> Result<Uuid> {
        let row = hosts
            .records()
            .iter()
            .find(|r| r.endpoint().label() == label)
            .context("fixture.route.host")?;
        Ok(graph
            .profiles()
            .iter()
            .find(|p| p.endpoint_id() == row.id())
            .context("fixture.route.profile")?
            .metadata()
            .id())
    };
    let outer_profile = profile_for("GUI_JUMP_OUTER")?;
    let middle_profile = profile_for("GUI_JUMP_MIDDLE")?;
    let leaf_profile = profile_for("GUI_HOST")?;
    let selected_key = russh::keys::PrivateKey::new(
        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
        "",
    )?;
    let agent_identity = if agent_route {
        use pulse_store::ssh_agent_identities::AgentIdentityEdit;
        let snapshot = store.load_ssh_agent_identities(Arc::new(|| true)).await?;
        let saved = store
            .edit_ssh_agent_identity(
                snapshot,
                AgentIdentityEdit::Create(pulse_core::ssh_agent_identity::SshAgentIdentity::new(
                    "GUI_ROUTE_AGENT",
                    "user",
                    "ssh-ed25519",
                    &selected_key
                        .public_key()
                        .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
                        .to_string(),
                )?),
                Arc::new(|| true),
            )
            .await?;
        Some(
            saved
                .records()
                .next()
                .context("fixture.route.agent_identity")?
                .id(),
        )
    } else {
        None
    };
    let mut source = store
        .load_ssh_profiles_authorized(Arc::new(|| true))
        .await?;
    for (id, label, via) in [
        (outer_profile, "Outer", None),
        (middle_profile, "Middle", Some(outer_profile)),
        (leaf_profile, "Leaf", Some(middle_profile)),
    ] {
        source = store
            .edit_ssh_profile_with_jump(
                source,
                ProfileEdit {
                    id,
                    label: label.into(),
                    username: "user".into(),
                    identity: Some(if id == outer_profile {
                        identity
                    } else {
                        agent_identity.unwrap_or(identity)
                    }),
                },
                via,
                Arc::new(|| true),
            )
            .await?;
    }
    ensure!(
        source.profiles().count() == 3,
        "fixture.route.profile_count"
    );
    let mut tasks = vec![];
    for (listener, target, label, seed) in [
        (middle, middle_address, "middle", 10u8),
        (outer, outer_address, "outer", 11u8),
    ] {
        let destination = if label == "middle" {
            leaf_address
        } else {
            middle_address
        };
        let key = russh::keys::PrivateKey::new(
            russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[seed; 32]).into(),
            "",
        )?;
        eprintln!(
            "fixture.route.{label}.fingerprint.{}",
            key.public_key()
                .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
        );
        let agent_key = if key_interactive_route || (agent_route && label == "middle") {
            Some(selected_key.public_key().clone())
        } else {
            None
        };
        tasks.push(tokio::spawn(async move {
            let mut server = Jump {
                target: destination,
                label,
                connection: false,
                agent_key,
                interactive_round: interactive_route.then_some(0),
                key_joint: key_interactive_route
                    || (std::env::args().nth(3).as_deref() == Some("agent-interactive-route")
                        && label == "middle"),
                key_factor_complete: false,
                password_joint: std::env::args().nth(3).as_deref()
                    == Some("password-interactive-route"),
                password_factor_complete: false,
            };
            eprintln!("fixture.route.{label}.listening.{}", target.port());
            let _ = server
                .run_on_socket(
                    Arc::new(server::Config {
                        keys: vec![key],
                        ..Default::default()
                    }),
                    &listener,
                )
                .await;
        }));
    }
    Ok(Servers { tasks })
}
