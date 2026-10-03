//! Isolated S4 protocol probe, never a production SSH connection entry point.
use anyhow::{Context, Result, ensure};
use russh::server::Server as _;
use russh::{
    Channel, ChannelId, ChannelMsg, Disconnect, Pty, client,
    keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, PublicKey, PublicKeyOrCertificate},
    server,
};
use serde_json::json;
use std::{
    process::Stdio,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{io::AsyncReadExt, net::TcpListener, sync::oneshot};
const USER: &str = "pulse-probe";
const PASSWORD: &str = "synthetic-test-password";
const EXEC: &[u8] = b"pulse-probe-fixed-exec";
const EXEC_BYTES: &[u8] = b"SSH_EXEC_OK\n";
#[derive(Default)]
struct Metrics {
    active: AtomicUsize,
    password_denied: AtomicUsize,
    password_accepted: AtomicUsize,
    public_accepted: AtomicUsize,
    ki_started: AtomicUsize,
    ki_denied: AtomicUsize,
    ki_accepted: AtomicUsize,
    ki_cancelled: AtomicUsize,
    pty: AtomicUsize,
    resize: Mutex<Option<(u32, u32)>>,
}
struct ProbeServer {
    metrics: Arc<Metrics>,
    allowed: PublicKey,
    connection: bool,
    has_pty: bool,
    challenge: bool,
}
impl Drop for ProbeServer {
    fn drop(&mut self) {
        if self.connection {
            if self.challenge {
                self.metrics.ki_cancelled.fetch_add(1, Ordering::SeqCst);
            }
            self.metrics.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl server::Server for ProbeServer {
    type Handler = Self;
    fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self {
        self.metrics.active.fetch_add(1, Ordering::SeqCst);
        Self {
            metrics: self.metrics.clone(),
            allowed: self.allowed.clone(),
            connection: true,
            has_pty: false,
            challenge: false,
        }
    }
}
impl server::Handler for ProbeServer {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        if user == USER && password == PASSWORD {
            self.metrics
                .password_accepted
                .fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Accept)
        } else {
            self.metrics.password_denied.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::reject())
        }
    }
    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _methods: &str,
        response: Option<server::Response<'a>>,
    ) -> std::result::Result<server::Auth, Self::Error> {
        if user != USER {
            self.challenge = false;
            return Ok(server::Auth::reject());
        }
        if let Some(mut answer) = response {
            let active = std::mem::replace(&mut self.challenge, false);
            let first = answer.next();
            let sole = answer.next().is_none();
            if active && sole && first.as_ref().is_some_and(|a| a.as_ref() == b"123456") {
                self.metrics.ki_accepted.fetch_add(1, Ordering::SeqCst);
                Ok(server::Auth::Accept)
            } else {
                self.metrics.ki_denied.fetch_add(1, Ordering::SeqCst);
                Ok(server::Auth::reject())
            }
        } else {
            self.challenge = true;
            self.metrics.ki_started.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Partial {
                name: "pulse-probe".into(),
                instructions: "synthetic challenge only".into(),
                prompts: std::borrow::Cow::Owned(vec![("Synthetic token: ".into(), false)]),
            })
        }
    }

    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        if user == USER && key == &self.allowed {
            self.metrics.public_accepted.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Accept)
        } else {
            Ok(server::Auth::reject())
        }
    }
    async fn channel_open_session(
        &mut self,
        _channel: Channel<server::Msg>,
        reply: server::ChannelOpenHandle,
        _session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
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
        _px: u32,
        _py: u32,
        _modes: &[(Pty, u32)],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if term == "xterm-256color" && (2..=160).contains(&cols) && (2..=60).contains(&rows) {
            self.has_pty = true;
            self.metrics.pty.fetch_add(1, Ordering::SeqCst);
            session.channel_success(channel)?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }
    async fn shell_request(
        &mut self,
        channel: ChannelId,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if self.has_pty {
            session.channel_success(channel)?;
            session.data(channel, b"SSH_READY\n".to_vec())?;
        } else {
            session.channel_failure(channel)?;
        }
        Ok(())
    }
    async fn window_change_request(
        &mut self,
        channel: ChannelId,
        cols: u32,
        rows: u32,
        _px: u32,
        _py: u32,
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if !self.has_pty || !(2..=160).contains(&cols) || !(2..=60).contains(&rows) {
            return Err(russh::Error::Disconnect);
        }
        *self
            .metrics
            .resize
            .lock()
            .map_err(|_| russh::Error::Disconnect)? = Some((cols, rows));
        session.data(channel, b"SSH_RESIZED\n".to_vec())?;
        Ok(())
    }
    async fn data(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if !self.has_pty || data.len() > 512 {
            return Err(russh::Error::Disconnect);
        }
        if data == b"exit" {
            session.data(channel, b"SSH_END\n".to_vec())?;
            session.exit_status_request(channel, 7)?;
            session.eof(channel)?;
            session.close(channel)?;
        } else {
            session.data(channel, data.to_vec())?;
        }
        Ok(())
    }
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if data != EXEC {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        session.data(channel, EXEC_BYTES.to_vec())?;
        session.exit_status_request(channel, 7)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}
fn host_matches(presented: &PublicKeyOrCertificate, expected: &PublicKey) -> bool {
    matches!(presented,PublicKeyOrCertificate::PublicKey{key,..}if key==expected)
}

pub(crate) struct PinnedClient {
    expected: PublicKey,
    mismatch: Arc<AtomicBool>,
}
impl client::Handler for PinnedClient {
    type Error = russh::Error;
    async fn check_server_key(
        &mut self,
        presented: &PublicKeyOrCertificate,
    ) -> std::result::Result<bool, Self::Error> {
        let accepted = host_matches(presented, &self.expected);
        if !accepted {
            self.mismatch.store(true, Ordering::SeqCst);
        }
        Ok(accepted)
    }
}
struct ListenerGuard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.stop.shutdown("test completed".into());
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
pub(crate) async fn connect(
    address: std::net::SocketAddr,
    key: PublicKey,
    mismatch: Arc<AtomicBool>,
) -> Result<client::Handle<PinnedClient>> {
    let config = Arc::new(client::Config {
        inactivity_timeout: Some(Duration::from_secs(5)),
        maximum_packet_size: 8192,
        ..Default::default()
    });
    Ok(tokio::time::timeout(
        Duration::from_secs(5),
        client::connect(
            config,
            address,
            PinnedClient {
                expected: key,
                mismatch,
            },
        ),
    )
    .await??)
}
pub(crate) async fn success(channel: &mut Channel<client::Msg>) -> Result<bool> {
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Success => return Ok(true),
                ChannelMsg::Failure => return Ok(false),
                _ => {}
            }
        }
        anyhow::bail!("channel closed before request receipt")
    })
    .await?
}
async fn receive(channel: &mut Channel<client::Msg>, expected: &[u8]) -> Result<()> {
    let mut bytes = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        while bytes.len() < expected.len() {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    ensure!(
                        bytes.len() + data.len() <= 1024,
                        "test channel exceeded byte bound"
                    );
                    bytes.extend_from_slice(&data);
                }
                Some(ChannelMsg::Failure) | None => anyhow::bail!("channel closed before marker"),
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(bytes == expected, "SSH bytes changed");
    Ok(())
}
async fn end(channel: &mut Channel<client::Msg>) -> Result<()> {
    let mut data = Vec::new();
    let mut code = None;
    let mut eof = false;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data: part } => {
                    ensure!(
                        data.len() + part.len() <= 1024,
                        "test end output exceeded bound"
                    );
                    data.extend_from_slice(&part);
                }
                ChannelMsg::ExitStatus { exit_status } => code = Some(exit_status),
                ChannelMsg::Eof => eof = true,
                ChannelMsg::Close => break,
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(
        data == b"SSH_END\n" && code == Some(7) && eof,
        "SSH EOF/status/drain mismatch"
    );
    Ok(())
}
async fn system_client(
    address: std::net::SocketAddr,
    host: &PublicKey,
    key: &PrivateKey,
) -> Result<serde_json::Value> {
    let dir = tempfile::tempdir()?;
    let identity = dir.path().join("identity");
    let known = dir.path().join("known_hosts");
    let empty = dir.path().join("empty_config");
    let encoded = key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?;
    std::fs::write(&identity, encoded.as_bytes())?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&identity, std::fs::Permissions::from_mode(0o600))?;
    }
    std::fs::write(
        &known,
        format!("[127.0.0.1]:{} {}\n", address.port(), host.to_openssh()?),
    )?;
    std::fs::write(&empty, "")?;
    let version = tokio::process::Command::new("ssh")
        .arg("-V")
        .output()
        .await
        .context("system OpenSSH client required")?;
    ensure!(version.stderr.len() <= 1024, "ssh version output bound");
    let version = String::from_utf8(version.stderr)?
        .split_whitespace()
        .next()
        .context("ssh version missing")?
        .to_string();
    ensure!(version.starts_with("OpenSSH_"), "not OpenSSH");
    let mut command = tokio::process::Command::new("ssh");
    command
        .args(["-F"])
        .arg(&empty)
        .args([
            "-T",
            "-o",
            "BatchMode=yes",
            "-o",
            "IdentitiesOnly=yes",
            "-o",
            "IdentityAgent=none",
            "-o",
            "StrictHostKeyChecking=yes",
            "-o",
            "HostKeyAlgorithms=ssh-ed25519",
            "-o",
            "ConnectTimeout=3",
            "-o",
        ])
        .arg(format!("UserKnownHostsFile={}", known.display()))
        .arg("-o")
        .arg(format!("GlobalKnownHostsFile={}", empty.display()))
        .arg("-i")
        .arg(&identity)
        .arg("-p")
        .arg(address.port().to_string())
        .arg(format!("{USER}@127.0.0.1"))
        .arg(std::str::from_utf8(EXEC)?)
        .env_remove("SSH_AUTH_SOCK")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let mut child = command.spawn()?;
    let stdout = child.stdout.take().context("missing SSH stdout")?;
    let read = tokio::spawn(async move {
        let mut output = Vec::new();
        stdout
            .take(1024)
            .read_to_end(&mut output)
            .await
            .map(|_| output)
    });
    let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(status) => status?,
        Err(_) => {
            child.start_kill()?;
            child.wait().await?;
            read.abort();
            anyhow::bail!("system SSH deadline");
        }
    };
    let output = tokio::time::timeout(Duration::from_secs(1), read).await???;
    ensure!(
        status.code() == Some(7) && output == EXEC_BYTES,
        "OpenSSH fixed exec/exit mismatch"
    );
    Ok(
        json!({"version":version,"publicKeyAuth":true,"strictIsolatedKnownHosts":true,"fixedExecOnly":true,"nativeChildExit":7,"stdoutDrained":true}),
    )
}
async fn probe() -> Result<serde_json::Value> {
    let host_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let user_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let wrong_key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
        .public_key()
        .clone();
    let expected = host_key.public_key().clone();
    let metrics = Arc::new(Metrics::default());
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let cfg = Arc::new(server::Config {
        keys: vec![host_key],
        inactivity_timeout: Some(Duration::from_secs(5)),
        auth_rejection_time: Duration::from_millis(20),
        auth_rejection_time_initial: Some(Duration::ZERO),
        maximum_packet_size: 8192,
        ..Default::default()
    });
    let mut server = ProbeServer {
        metrics: metrics.clone(),
        allowed: user_key.public_key().clone(),
        connection: false,
        has_pty: false,
        challenge: false,
    };
    let (handle_tx, handle_rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let running = server.run_on_socket(cfg, &listener);
        let _ = handle_tx.send(running.handle());
        running.await
    });
    let stop = handle_rx.await?;
    let mut guard = ListenerGuard {
        stop,
        task: Some(task),
    };
    let rejected = Arc::new(AtomicBool::new(false));
    let bad = connect(address, wrong_key, rejected.clone()).await;
    ensure!(
        bad.is_err() && rejected.load(Ordering::SeqCst),
        "changed host key not explicitly rejected"
    );
    ensure!(
        metrics.password_accepted.load(Ordering::SeqCst) == 0
            && metrics.password_denied.load(Ordering::SeqCst) == 0,
        "authentication occurred before host trust"
    );
    let trusted = Arc::new(AtomicBool::new(false));
    let mut session = connect(address, expected.clone(), trusted.clone()).await?;
    ensure!(
        !session
            .authenticate_password(USER, "wrong-synthetic-password")
            .await?
            .success(),
        "wrong password accepted"
    );
    ensure!(
        session
            .authenticate_password(USER, PASSWORD)
            .await?
            .success(),
        "password auth rejected"
    );
    let mut channel = session.channel_open_session().await?;
    channel
        .request_pty(true, "xterm-256color", 80, 24, 0, 0, &[])
        .await?;
    ensure!(success(&mut channel).await?, "PTY request rejected");
    channel.request_shell(true).await?;
    ensure!(success(&mut channel).await?, "shell request rejected");
    receive(&mut channel, b"SSH_READY\n").await?;
    channel.window_change(91, 37, 0, 0).await?;
    receive(&mut channel, b"SSH_RESIZED\n").await?;
    ensure!(
        *metrics
            .resize
            .lock()
            .map_err(|_| anyhow::anyhow!("resize metric poisoned"))?
            == Some((91, 37)),
        "window dimensions not observed"
    );
    let payload = "literal ; $(not-a-command) 中文🌍\r\n";
    channel.data(payload.as_bytes()).await?;
    receive(&mut channel, payload.as_bytes()).await?;
    channel.data(&b"exit"[..]).await?;
    end(&mut channel).await?;
    session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    let mut pub_session =
        connect(address, expected.clone(), Arc::new(AtomicBool::new(false))).await?;
    ensure!(
        pub_session
            .authenticate_publickey(
                USER,
                PrivateKeyWithHashAlg::new(Arc::new(user_key.clone()), None)
            )
            .await?
            .success(),
        "publickey auth rejected"
    );
    let mut denied = pub_session.channel_open_session().await?;
    denied.exec(true, b"not-approved".to_vec()).await?;
    ensure!(!success(&mut denied).await?, "arbitrary exec accepted");
    denied.close().await?;
    pub_session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    keyboard_interactive(address, &expected, &metrics).await?;
    let openssh = system_client(address, &expected, &user_key).await?;
    guard.stop.shutdown("test complete".into());
    if let Some(task) = &mut guard.task {
        tokio::time::timeout(Duration::from_secs(3), task).await???;
    }
    guard.task = None;
    tokio::time::timeout(Duration::from_secs(3), async {
        while metrics.active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("SSH connections did not drain")?;
    ensure!(
        metrics.password_denied.load(Ordering::SeqCst) == 1
            && metrics.password_accepted.load(Ordering::SeqCst) == 1
            && metrics.public_accepted.load(Ordering::SeqCst) > 0,
        "authentication evidence missing"
    );
    Ok(
        json!({"schemaVersion":1,"russh":"0.63.3","profile":"optional xtask ring/ed25519 probe; no RSA/compression/legacy feature validation","loopbackOnly":true,"changedHostKeyRejectedBeforeAuth":true,"wrongPasswordRejected":true,"passwordAuth":true,"publicKeyAuth":true,"keyboardInteractive":{"syntheticChallenge":true,"wrongAnswerRejected":true,"singleChallengeConsumed":true,"cancelledConnectionDrained":true,"noRealMfaHardware":true},"ptyRequestAcknowledged":true,"windowChangeObserved":[91,37],"utf8BinaryRoundtrip":true,"exitStatus":7,"eofAndOutputDrain":true,"arbitraryExecRejected":true,"connectionsAndListenerJoined":true,"systemClient":openssh,"limitations":["Controlled russh server, not real OS PTY or server SSH implementation matrix","OpenSSH 7-9, dropbear, old devices, real MFA/UI prompt integration, certificates, jumps, forwarding, Agent/FIDO2 and GBK are not yet accepted","No production SSH transport, known_hosts storage, user prompts or credential-vault integration"],"platform":std::env::consts::OS}),
    )
}
fn validate_challenge(reply: &client::KeyboardInteractiveAuthResponse) -> Result<()> {
    match reply {
        client::KeyboardInteractiveAuthResponse::InfoRequest {
            name,
            instructions,
            prompts,
        } => {
            ensure!(
                name == "pulse-probe"
                    && instructions == "synthetic challenge only"
                    && prompts.len() == 1
                    && prompts[0].prompt == "Synthetic token: "
                    && !prompts[0].echo,
                "unknown/unbounded challenge"
            );
            Ok(())
        }
        _ => anyhow::bail!("missing keyboard-interactive prompt"),
    }
}
async fn keyboard_interactive(
    address: std::net::SocketAddr,
    key: &PublicKey,
    metrics: &Metrics,
) -> Result<()> {
    use client::KeyboardInteractiveAuthResponse as Reply;
    let mut connection = connect(address, key.clone(), Arc::new(AtomicBool::new(false))).await?;
    let prompt = tokio::time::timeout(
        Duration::from_secs(3),
        connection.authenticate_keyboard_interactive_start(USER, None),
    )
    .await??;
    validate_challenge(&prompt)?;
    ensure!(
        matches!(
            tokio::time::timeout(
                Duration::from_secs(3),
                connection
                    .authenticate_keyboard_interactive_respond(vec!["wrong-synthetic".into()])
            )
            .await??,
            Reply::Failure {
                partial_success: false,
                ..
            }
        ),
        "wrong challenge response accepted"
    );
    let retry = tokio::time::timeout(
        Duration::from_secs(3),
        connection.authenticate_keyboard_interactive_start(USER, None),
    )
    .await??;
    validate_challenge(&retry)?;
    ensure!(
        matches!(
            tokio::time::timeout(
                Duration::from_secs(3),
                connection.authenticate_keyboard_interactive_respond(vec!["123456".into()])
            )
            .await??,
            Reply::Success
        ),
        "correct synthetic challenge rejected"
    );
    connection
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    let mut cancelled = connect(address, key.clone(), Arc::new(AtomicBool::new(false))).await?;
    let prompt = tokio::time::timeout(
        Duration::from_secs(3),
        cancelled.authenticate_keyboard_interactive_start(USER, None),
    )
    .await??;
    validate_challenge(&prompt)?;
    cancelled
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(cancelled);
    tokio::time::timeout(Duration::from_secs(3), async {
        while metrics.ki_cancelled.load(Ordering::SeqCst) != 1 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("cancelled challenge connection did not drain independently")?;
    ensure!(
        metrics.ki_started.load(Ordering::SeqCst) == 3
            && metrics.ki_denied.load(Ordering::SeqCst) == 1
            && metrics.ki_accepted.load(Ordering::SeqCst) == 1,
        "keyboard-interactive metrics mismatch"
    );
    Ok(())
}

fn validate_report(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["russh"] == "0.63.3",
        "SSH report version"
    );
    for field in [
        "loopbackOnly",
        "changedHostKeyRejectedBeforeAuth",
        "wrongPasswordRejected",
        "passwordAuth",
        "publicKeyAuth",
        "ptyRequestAcknowledged",
        "utf8BinaryRoundtrip",
        "eofAndOutputDrain",
        "arbitraryExecRejected",
        "connectionsAndListenerJoined",
    ] {
        ensure!(r[field] == true, "missing SSH observation: {field}");
    }
    for field in [
        "syntheticChallenge",
        "wrongAnswerRejected",
        "singleChallengeConsumed",
        "cancelledConnectionDrained",
        "noRealMfaHardware",
    ] {
        ensure!(
            r["keyboardInteractive"][field] == true,
            "missing keyboard-interactive evidence"
        );
    }
    ensure!(
        r["exitStatus"] == 7 && r["windowChangeObserved"] == json!([91, 37]),
        "SSH exit/window evidence"
    );
    for field in [
        "publicKeyAuth",
        "strictIsolatedKnownHosts",
        "fixedExecOnly",
        "stdoutDrained",
    ] {
        ensure!(
            r["systemClient"][field] == true,
            "missing OpenSSH observation"
        );
    }
    ensure!(
        r["systemClient"]["nativeChildExit"] == 7
            && r["systemClient"]["version"]
                .as_str()
                .is_some_and(|s| s.starts_with("OpenSSH_") && s.len() < 64),
        "OpenSSH identity/exit evidence"
    );
    ensure!(
        r["limitations"].as_array().is_some_and(|a| !a.is_empty()),
        "missing S4 limitations"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    std::fs::create_dir_all(root.join("target/probes"))?;
    let destination = root.join("target/probes/ssh.json");
    if destination.exists() {
        std::fs::remove_file(&destination)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(25), probe()).await })??;
    validate_report(&report)?;
    std::fs::write(destination, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS isolated SSH host-key/auth/PTY/UTF8/EOF and system OpenSSH client");
    Ok(())
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn synthetic_challenge_rejects_unknown_name_extra_prompts_and_echo() {
        use client::{KeyboardInteractiveAuthResponse as Reply, Prompt};
        let prompt = || Prompt {
            prompt: "Synthetic token: ".into(),
            echo: false,
        };
        let good = Reply::InfoRequest {
            name: "pulse-probe".into(),
            instructions: "synthetic challenge only".into(),
            prompts: vec![prompt()],
        };
        assert!(validate_challenge(&good).is_ok());
        for bad in [
            Reply::InfoRequest {
                name: "foreign".into(),
                instructions: "synthetic challenge only".into(),
                prompts: vec![prompt()],
            },
            Reply::InfoRequest {
                name: "pulse-probe".into(),
                instructions: "synthetic challenge only".into(),
                prompts: vec![prompt(), prompt()],
            },
            Reply::InfoRequest {
                name: "pulse-probe".into(),
                instructions: "synthetic challenge only".into(),
                prompts: vec![Prompt {
                    prompt: "Synthetic token: ".into(),
                    echo: true,
                }],
            },
            Reply::Success,
        ] {
            assert!(validate_challenge(&bad).is_err());
        }
    }

    #[test]
    fn pinned_ed25519_key_equality_is_required() {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let wrong = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        assert!(host_matches(
            &key.public_key().clone().into(),
            key.public_key()
        ));
        assert!(!host_matches(
            &wrong.public_key().clone().into(),
            key.public_key()
        ));
    }
    #[test]
    fn success_flag_alone_is_not_an_ssh_report() {
        assert!(validate_report(&json!({"schemaVersion":1,"completed":true})).is_err());
        assert!(validate_report(&json!({})).is_err());
    }
    #[tokio::test]
    async fn test_auth_requires_both_expected_username_and_credential() {
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let wrong = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let mut server = ProbeServer {
            metrics: Arc::new(Metrics::default()),
            allowed: key.public_key().clone(),
            connection: false,
            has_pty: false,
            challenge: false,
        };
        use russh::server::Handler as _;
        assert!(matches!(
            server.auth_password(USER, PASSWORD).await.unwrap(),
            server::Auth::Accept
        ));
        assert!(!matches!(
            server.auth_password("wrong", PASSWORD).await.unwrap(),
            server::Auth::Accept
        ));
        assert!(!matches!(
            server
                .auth_publickey(USER, wrong.public_key())
                .await
                .unwrap(),
            server::Auth::Accept
        ));
        assert!(!matches!(
            server
                .auth_publickey("wrong", key.public_key())
                .await
                .unwrap(),
            server::Auth::Accept
        ));
        assert!(matches!(
            server.auth_publickey(USER, key.public_key()).await.unwrap(),
            server::Auth::Accept
        ));
    }
}
