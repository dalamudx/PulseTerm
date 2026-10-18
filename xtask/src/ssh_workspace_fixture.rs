//! Dev-only isolated real SSH peer and private ciphertext seed for native GUI acceptance.
use super::*;
#[path = "ssh_workspace_agent_fixture.rs"]
mod agent_fixture;
#[path = "ssh_workspace_interactive_fixture.rs"]
mod interactive_fixture;
#[path = "ssh_workspace_key_fixture.rs"]
mod key_fixture;
#[path = "ssh_workspace_password_fixture.rs"]
mod password_fixture;
#[path = "ssh_workspace_route_fixture.rs"]
mod route_fixture;
struct GuiPeer {
    auth: AcceptSpy,
    pty: bool,
    line: Vec<u8>,
    accepted: bool,
    public_key: Option<russh::keys::PublicKey>,
    interactive_round: Option<u8>,
    password_factor_complete: bool,
    key_factor_complete: bool,
}
impl Drop for GuiPeer {
    fn drop(&mut self) {
        if std::env::var_os("PULSE_SSH_PASSWORD_MFA_POLICY").is_some() {
            eprintln!("fixture.ssh_password_joint_stream_exited");
        }
        if std::env::var_os("PULSE_SSH_KEY_MFA_POLICY").is_some() {
            eprintln!("fixture.ssh_key_joint_stream_exited");
        }
        if self.interactive_round.is_some_and(|round| round > 0) {
            eprintln!("fixture.ssh_interactive_stream_exited");
        }
        if self.accepted {
            eprintln!("fixture.ssh_authenticated_stream_exited");
        }
    }
}
impl Server for GuiPeer {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        Self {
            auth: AcceptSpy {
                auth: self.auth.auth.clone(),
                exact: self.auth.exact.clone(),
            },
            pty: false,
            line: Vec::new(),
            accepted: false,
            public_key: self.public_key.clone(),
            interactive_round: self.interactive_round.map(|_| 0),
            password_factor_complete: false,
            key_factor_complete: false,
        }
    }
}
impl server::Handler for GuiPeer {
    type Error = russh::Error;
    async fn auth_none(&mut self, user: &str) -> Result<server::Auth, Self::Error> {
        if self.public_key.is_some() || self.interactive_round.is_some() {
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        } else {
            server::Handler::auth_none(&mut self.auth, user).await
        }
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        if let Some(policy) = std::env::var_os("PULSE_SSH_PASSWORD_MFA_POLICY") {
            return Ok(password_fixture::authenticate(
                self,
                user,
                password,
                std::path::Path::new(&policy),
            ));
        }
        if self.public_key.is_some() || self.interactive_round.is_some() {
            eprintln!("fixture.ssh_password_auth_attempt_rejected");
            return Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            });
        }
        let result = server::Handler::auth_password(&mut self.auth, user, password).await?;
        if matches!(result, server::Auth::Accept) {
            self.accepted = true;
            eprintln!("fixture.ssh_password_authenticated");
        }
        Ok(result)
    }
    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _: &str,
        response: Option<server::Response<'a>>,
    ) -> Result<server::Auth, Self::Error> {
        if std::env::var_os("PULSE_SSH_PASSWORD_MFA_POLICY").is_some()
            && !self.password_factor_complete
        {
            eprintln!("fixture.ssh_password_joint_premature_mfa_rejected");
            return Ok(server::Auth::reject());
        }
        if std::env::var_os("PULSE_SSH_KEY_MFA_POLICY").is_some() && !self.key_factor_complete {
            eprintln!("fixture.ssh_key_joint_premature_mfa_rejected");
            return Ok(server::Auth::reject());
        }
        interactive_fixture::respond(self, user, response)
    }
    async fn auth_publickey_offered(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        if user == "user" && self.public_key.as_ref() == Some(key) {
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &russh::keys::PublicKey,
    ) -> Result<server::Auth, Self::Error> {
        if user == "user" && self.public_key.as_ref() == Some(key) {
            if let Some(path) = std::env::var_os("PULSE_SSH_KEY_MFA_POLICY") {
                match std::fs::read_to_string(path).as_deref() {
                    Ok("partial") => {
                        self.key_factor_complete = true;
                        eprintln!("fixture.ssh_key_joint_partial_success");
                        return Ok(server::Auth::Reject {
                            proceed_with_methods: Some(
                                (&[russh::MethodKind::KeyboardInteractive][..]).into(),
                            ),
                            partial_success: true,
                        });
                    }
                    Ok("complete") => eprintln!("fixture.ssh_key_joint_complete_key_success"),
                    _ => {
                        eprintln!("fixture.ssh_key_joint_key_rejected");
                        return Ok(server::Auth::reject());
                    }
                }
            }
            self.auth.auth.fetch_add(1, Ordering::SeqCst);
            self.auth.exact.store(true, Ordering::SeqCst);
            self.accepted = true;
            eprintln!("fixture.ssh_public_key_authenticated");
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::Reject {
                proceed_with_methods: None,
                partial_success: false,
            })
        }
    }
    async fn channel_open_session(
        &mut self,
        _: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        reply.accept().await;
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn pty_request(
        &mut self,
        channel: ChannelId,
        term: &str,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &[(Pty, u32)],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        let scope_probe = std::env::var_os("PULSE_SSH_SCOPE_PROBE").is_some();
        let allowed_term =
            term == "xterm-256color" || (scope_probe && matches!(term, "xterm" | "vt100"));
        self.pty = allowed_term && (2..=160).contains(&cols) && (2..=60).contains(&rows);
        if self.pty && scope_probe {
            eprintln!("fixture.ssh_scope_term.{term}");
        }
        if self.pty {
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    async fn window_change_request(
        &mut self,
        _: ChannelId,
        cols: u32,
        rows: u32,
        _: u32,
        _: u32,
        _: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if !(2..=160).contains(&cols) || !(2..=60).contains(&rows) {
            return Err(russh::Error::Disconnect);
        }
        eprintln!("fixture.ssh_window_request.{cols}.{rows}");
        Ok(()) // intentionally no reply; not an OS/kernel confirmation.
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if std::env::var_os("PULSE_SSH_PASSWORD_MFA_POLICY").is_some() {
            eprintln!("fixture.ssh_password_joint_shell_requested");
        }
        if std::env::var_os("PULSE_SSH_KEY_MFA_POLICY").is_some() {
            eprintln!("fixture.ssh_key_joint_shell_requested");
        }
        if !self.pty {
            return Err(russh::Error::Disconnect);
        }
        session.channel_success(channel)?;
        session.data(channel, b"GUI_SSH_READY\r\n".to_vec())?;
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> Result<(), Self::Error> {
        if !self.pty || self.line.len().saturating_add(data.len()) > 512 {
            return Err(russh::Error::Disconnect);
        }
        let scope_probe = std::env::var_os("PULSE_SSH_SCOPE_PROBE").is_some();
        if scope_probe {
            eprintln!("fixture.ssh_scope_input_received");
        }
        for byte in data {
            if scope_probe && *byte == 7 && self.line.starts_with(b"\x1b]10;rgb:") {
                let preset = match self.line.as_slice() {
                    b"\x1b]10;rgb:ffff/ffff/ffff" => "classic",
                    b"\x1b]10;rgb:e2e2/e8e8/f0f0" => "midnight",
                    b"\x1b]10;rgb:1f1f/2929/3737" => "paper",
                    _ => return Err(russh::Error::Disconnect),
                };
                eprintln!("fixture.ssh_scope_color.{preset}");
                self.line.clear();
                session.data(channel, b"GUI_SCOPE_COLOR_OK\r\n".to_vec())?;
                continue;
            }
            if *byte == b'\r' || *byte == b'\n' {
                if self.line.is_empty() {
                    continue;
                }
                if self.line == b"exit" {
                    session.data(channel, b"GUI_SSH_END\r\n".to_vec())?;
                    session.exit_status_request(channel, 7)?;
                    eprintln!("fixture.ssh_remote_exit_status_sent.7");
                    session.eof(channel)?;
                    session.close(channel)?;
                    self.line.clear();
                } else if scope_probe && self.line == b"SCOPE_QUERY" {
                    self.line.clear();
                    session.data(channel, b"\x1b]10;?\x07".to_vec())?;
                } else if scope_probe && self.line == b"SCOPE_HISTORY" {
                    self.line.clear();
                    let mut output = format!(
                        "SOFT{}中éNEEDLE\r\nHARD_BREAK_A\r\nHARD_BREAK_B\r\n",
                        "0123456789".repeat(20)
                    );
                    for row in 0..120 {
                        output.push_str(&format!("HISTORY_ROW_{row:03}\r\n"));
                    }
                    session.data(channel, output.into_bytes())?;
                    eprintln!("fixture.ssh_scope_history_sent");
                } else if scope_probe && self.line == b"\x08\x7f" {
                    self.line.clear();
                    eprintln!("fixture.ssh_scope_keys.bs_del");
                    session.data(channel, b"GUI_SCOPE_KEYS_OK\r\n".to_vec())?;
                } else if scope_probe && self.line == b"\x7f\x1b[3~" {
                    self.line.clear();
                    eprintln!("fixture.ssh_scope_keys.del_csi");
                    session.data(channel, b"GUI_SCOPE_KEYS_OK\r\n".to_vec())?;
                } else {
                    if std::env::var_os("PULSE_SSH_ROUTE").is_some() && self.line == b"R" {
                        eprintln!("fixture.route_input_R_confirmed");
                    }
                    if std::env::var_os("PULSE_SSH_AGENT").is_some() && self.line == b"A" {
                        eprintln!("fixture.agent_input_A_confirmed");
                    }
                    let mut echo = b"GUI_SSH_ECHO:".to_vec();
                    echo.append(&mut self.line);
                    echo.extend_from_slice(b"\r\n");
                    session.data(channel, echo)?;
                    eprintln!("fixture.ssh_input_line_echoed");
                }
            } else {
                self.line.push(*byte)
            }
        }
        Ok(())
    }
}
fn seed_root(directory: &std::path::Path) -> Result<&std::path::Path> {
    use std::path::Path;
    ensure!(
        directory.is_absolute()
            && directory
                .file_name()
                .is_some_and(|name| name == "dev.pulseterm.workspace-preview"),
        "fixture.only_owned_temp_target_allowed"
    );
    let data = directory.parent().context("fixture.invalid_temp_target")?;
    ensure!(
        data.file_name().is_some_and(|name| name == "data"),
        "fixture.invalid_temp_target"
    );
    let root = data.parent().context("fixture.invalid_temp_target")?;
    let suffix = root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| name.strip_prefix("pulseterm-workspace-ssh-session."))
        .context("fixture.invalid_temp_target")?;
    ensure!(
        root.parent() == Some(Path::new("/data/temp"))
            && suffix.len() == 6
            && suffix.bytes().all(|byte| byte.is_ascii_alphanumeric()),
        "fixture.only_owned_temp_target_allowed"
    );
    Ok(root)
}
fn require_owned_seed_directory(directory: &std::path::Path) -> Result<()> {
    use std::os::unix::fs::MetadataExt;
    let root = seed_root(directory)?;
    let root_meta = std::fs::symlink_metadata(root)?;
    ensure!(
        root_meta.is_dir() && !root_meta.file_type().is_symlink(),
        "fixture.invalid_temp_owner"
    );
    // Reuse current-user/private-0700 chain validation without changing pre-existing permissions.
    pulse_store::state_directory::prepare(root)?;
    let marker = root.join("owned-workspace-ssh-session.fixture");
    let meta = std::fs::symlink_metadata(&marker)?;
    ensure!(
        meta.is_file()
            && !meta.file_type().is_symlink()
            && meta.uid() == root_meta.uid()
            && meta.nlink() == 1
            && meta.mode() & 0o777 == 0o600
            && meta.len() == 16,
        "fixture.invalid_temp_marker"
    );
    ensure!(
        std::fs::read(&marker)? == b"owned-fixture-v1",
        "fixture.invalid_temp_marker"
    );
    Ok(())
}

pub(crate) fn run() -> Result<()> {
    let directory = std::env::args()
        .nth(2)
        .map(std::path::PathBuf::from)
        .context("fixture.directory_required")?;
    require_owned_seed_directory(&directory)?;
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .max_blocking_threads(4)
        .build()?
        .block_on(async move {
            let dir = pulse_store::state_directory::prepare(&directory)?;
            if std::env::args().nth(3).as_deref() == Some("mutate-host") {
                // Independent-process CAS mutation of this task-owned existing source only.
                ensure!(dir.join("state.db").is_file(), "fixture.source_missing");
                let (store, worker) = StateStore::open(dir.join("state.db")).await?;
                let hosts = store.load_ssh_hosts().await?;
                ensure!(hosts.records().len() == 1, "fixture.single_host_required");
                let row = hosts.records().first().context("fixture.host_missing")?;
                let endpoint = row.endpoint();
                ensure!(
                    endpoint.label() == "GUI_HOST"
                        && endpoint.target().host() == "127.0.0.1"
                        && endpoint.username() == "user",
                    "fixture.not_seeded_host"
                );
                let edit = HostEdit::Update(
                    row.id(),
                    SshEndpoint::new(
                        "GUI_HOST_CHANGED",
                        endpoint.target().host(),
                        endpoint.target().port(),
                        endpoint.username(),
                    )?,
                );
                store.edit_ssh_hosts(hosts, edit, Arc::new(|| true)).await?;
                drop(store);
                worker.join().await?;
                eprintln!("fixture.ssh_gui_source_changed");
                return Ok(());
            }
            ensure!(
                !dir.join("state.db").exists(),
                "fixture.existing_database_refused"
            );
            let (store, worker) = StateStore::open(dir.join("state.db")).await?;
            let registry = SessionRegistry::new();
            let documents = ProductDocuments::new(registry.clone());
            let window = registry.open_window()?;
            documents.install(window)?;
            documents.started(window)?;
            let nonce = documents.finished(window)?;
            documents.confirm(window, nonce)?;
            let page = documents.current(window, nonce)?;
            let owner = PageVaultOwner::new(registry.clone());
            let empty = store.load_vault_ciphertext(Arc::new(|| true)).await?;
            let operation = owner.begin(page.clone())?;
            let (vault, creation) = operation.begin_creation(
                PasswordInput::new(b"fixed-target-master".to_vec())?,
                empty.clone(),
            )?;
            operation
                .persist_creation(&store, empty, vault, creation.ready().await?)
                .await?;
            let saved = store.load_vault_ciphertext(Arc::new(|| true)).await?;
            let unlock = operation.begin_unlock(
                PasswordInput::new(b"fixed-target-master".to_vec())?,
                saved.clone(),
            )?;
            operation
                .confirm_unlock(&store, saved, unlock.ready().await?)
                .await?
                .accept()?;
            let (id, private_public) = if matches!(
                std::env::args().nth(3).as_deref(),
                Some("cold-agent" | "agent-interactive")
            ) {
                let (id, public) = agent_fixture::seed_saved_identity(&store).await?;
                (id, Some(public))
            } else if matches!(
                std::env::args().nth(3).as_deref(),
                Some("key" | "key-stall" | "agent" | "key-interactive" | "key-interactive-route")
            ) {
                let (id, public) = key_fixture::seed(&owner, page.clone(), &store, vault).await?;
                (id, Some(public))
            } else {
                let identities = store
                    .edit_ssh_identities(
                        store.load_ssh_identities(Arc::new(|| true)).await?,
                        IdentityEdit::Create(SshPasswordIdentity::new(
                            "GUI_IDENTITY",
                            "user",
                            SecretRef::new(vault, Uuid::now_v7(), SecretKind::SshPassword, false)?,
                        )?),
                        Arc::new(|| true),
                    )
                    .await?;
                let id = identities
                    .records()
                    .next()
                    .context("fixture.identity_missing")?
                    .id();
                if std::env::args().nth(3).as_deref() != Some("interactive") {
                    owner
                        .begin(page.clone())?
                        .prepare_password_identity_write(
                            store.load_identity_vault_source(Arc::new(|| true)).await?,
                            id,
                        )?
                        .write(
                            &store,
                            SecretBytes::from_owned(b"fixed-target-secret".to_vec())?,
                        )
                        .await?;
                }
                (id, None)
            };
            let key = russh::keys::PrivateKey::new(
                russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[9; 32]).into(),
                "",
            )?;
            let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
            let address = listener.local_addr()?;
            let target = HostTarget::new("127.0.0.1", address.port())?;
            let _hosts = store
                .edit_ssh_hosts(
                    store.load_ssh_hosts().await?,
                    HostEdit::Create(SshEndpoint::new(
                        "GUI_HOST",
                        "127.0.0.1",
                        address.port(),
                        "user",
                    )?),
                    Arc::new(|| true),
                )
                .await?;
            let agent_route = matches!(
                std::env::args().nth(3).as_deref(),
                Some("agent-route" | "agent-interactive-route")
            );
            let private_public = if agent_route {
                Some(
                    russh::keys::PrivateKey::new(
                        russh::keys::ssh_key::private::Ed25519Keypair::from_seed(&[3; 32]).into(),
                        "",
                    )?
                    .public_key()
                    .clone(),
                )
            } else {
                private_public
            };
            let route_servers = if matches!(
                std::env::args().nth(3).as_deref(),
                Some(
                    "route"
                        | "agent-route"
                        | "interactive-route"
                        | "key-interactive-route"
                        | "agent-interactive-route"
                )
            ) {
                Some(
                    route_fixture::seed(
                        &store,
                        &listener,
                        id,
                        agent_route,
                        matches!(
                            std::env::args().nth(3).as_deref(),
                            Some(
                                "interactive-route"
                                    | "key-interactive-route"
                                    | "agent-interactive-route"
                            )
                        ),
                    )
                    .await?,
                )
            } else {
                None
            };
            if std::env::var_os("PULSE_SAVED_ALIAS").as_deref() == Some(std::ffi::OsStr::new("1")) {
                let snapshot = store
                    .load_ssh_profiles_authorized(Arc::new(|| true))
                    .await?;
                let selected = snapshot
                    .profiles()
                    .find(|p| p.endpoint_id() == _hosts.records()[0].id())
                    .context("fixture.alias.profile")?;
                let profile = selected.metadata().id();
                let default_identity = if agent_route {
                    selected
                        .identity_id()
                        .context("fixture.alias.agent_default")?
                } else {
                    id
                };
                store
                    .edit_ssh_profile(
                        snapshot,
                        pulse_store::ssh_profiles::ProfileEdit {
                            id: profile,
                            label: "Saved CLI Alias".into(),
                            username: "user".into(),
                            identity: if std::env::var("PULSE_SAVED_ALIAS_CASE").as_deref()
                                == Ok("missing-default")
                            {
                                None
                            } else {
                                Some(default_identity)
                            },
                        },
                        Arc::new(|| true),
                    )
                    .await?;
                if std::env::var("PULSE_SAVED_ALIAS_CASE").as_deref() == Ok("ambiguous") {
                    store
                        .edit_ssh_hosts(
                            store.load_ssh_hosts().await?,
                            HostEdit::Create(SshEndpoint::new(
                                "GUI_HOST",
                                "127.0.0.1",
                                address.port(),
                                "user",
                            )?),
                            Arc::new(|| true),
                        )
                        .await?;
                }
            }
            let _trust = store
                .edit_ssh_trust(
                    store.load_ssh_trust().await?,
                    pin_edit(&target, key.public_key())?,
                )
                .await?;
            owner.close()?;
            documents.remove(window)?;
            registry.close_window(window)?;
            registry.shutdown()?;
            drop(store);
            worker.join().await?;
            let auth = Arc::new(AtomicUsize::new(0));
            let exact = Arc::new(std::sync::atomic::AtomicBool::new(false));
            let mut peer = GuiPeer {
                auth: AcceptSpy {
                    auth: auth.clone(),
                    exact: exact.clone(),
                },
                pty: false,
                line: Vec::new(),
                accepted: false,
                public_key: private_public,
                interactive_round: matches!(
                    std::env::args().nth(3).as_deref(),
                    Some(
                        "interactive"
                            | "interactive-route"
                            | "key-interactive"
                            | "key-interactive-route"
                            | "password-interactive"
                            | "agent-interactive"
                            | "agent-interactive-route"
                    )
                )
                .then_some(0),
                password_factor_complete: false,
                key_factor_complete: false,
            };
            eprintln!(
                "fixture.ssh_gui_host_id.{}",
                _hosts
                    .records()
                    .first()
                    .context("fixture.host_missing")?
                    .id()
            );
            eprintln!("fixture.ssh_gui_identity_id.{id}");
            eprintln!("fixture.ssh_gui_seeded_listening");
            if matches!(
                std::env::args().nth(3).as_deref(),
                Some("stall" | "key-stall")
            ) {
                // This controlled TCP peer never sends KEX/key proof: no credential
                // authentication is possible. Observe real owned stream EOF, not a bool.
                for _ in 0..2 {
                    let (mut stream, _) = listener.accept().await?;
                    eprintln!("fixture.ssh_handshake_stream_owned");
                    tokio::io::AsyncWriteExt::write_all(
                        &mut stream,
                        b"SSH-2.0-PulseTermFixedFixture\r\n",
                    )
                    .await?;
                    let mut bytes = [0u8; 1024];
                    let mut seen = 0usize;
                    loop {
                        let count = tokio::io::AsyncReadExt::read(&mut stream, &mut bytes).await?;
                        if count == 0 {
                            break;
                        }
                        seen = seen.saturating_add(count);
                        ensure!(seen <= 65536, "fixture.handshake_input_budget");
                    }
                    eprintln!("fixture.ssh_handshake_stream_exited");
                }
                std::future::pending::<()>().await;
            }
            let _route_servers = route_servers;
            let _agent = if matches!(
                std::env::args().nth(3).as_deref(),
                Some(
                    "agent"
                        | "agent-route"
                        | "cold-agent"
                        | "agent-interactive"
                        | "agent-interactive-route"
                )
            ) {
                Some(agent_fixture::start(seed_root(&dir)?.join("agent.sock"))?)
            } else {
                None
            };
            eprintln!(
                "fixture.ssh_gui_fingerprint.{}",
                key.public_key()
                    .fingerprint(russh::keys::ssh_key::HashAlg::Sha256)
            );
            peer.run_on_socket(
                Arc::new(server::Config {
                    keys: vec![key],
                    ..Default::default()
                }),
                &listener,
            )
            .await?;
            Ok(())
        })
}
#[cfg(test)]
mod seed_directory_tests {
    use super::*;
    #[test]
    fn only_exact_owned_temp_root_and_app_directory_shape_are_candidates() -> Result<()> {
        use std::path::Path;
        let valid = Path::new(
            "/data/temp/pulseterm-workspace-ssh-session.A1b2C3/data/dev.pulseterm.workspace-preview",
        );
        ensure!(
            seed_root(valid)? == Path::new("/data/temp/pulseterm-workspace-ssh-session.A1b2C3")
        );
        for path in [
            "/tmp/pulseterm-workspace-ssh-session.A1b2C3/data/dev.pulseterm.workspace-preview",
            "/data/temp/foreign.A1b2C3/data/dev.pulseterm.workspace-preview",
            "/data/temp/pulseterm-workspace-ssh-session.A1b2C3/data/other-app",
            "/data/temp/pulseterm-workspace-ssh-session.A1b2C3/other/dev.pulseterm.workspace-preview",
            "/data/temp/pulseterm-workspace-ssh-session.A1b2C3/../data/dev.pulseterm.workspace-preview",
            "target/workspace/data/dev.pulseterm.workspace-preview",
        ] {
            ensure!(
                seed_root(Path::new(path)).is_err(),
                "fixture.path_boundary_accepted_foreign_target"
            );
        }
        Ok(())
    }
}
