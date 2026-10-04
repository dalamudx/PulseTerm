//! Private software agent test. Never inspect or mutate the user's Agent.
use anyhow::{Context, Result, ensure};
use russh::server::Server as _;
use russh::{
    Channel, ChannelId, ChannelMsg, Disconnect,
    keys::{
        Algorithm, PrivateKey, PublicKey,
        agent::{AgentIdentity, client::AgentClient},
    },
    server,
};
use serde_json::json;
use std::{
    os::unix::fs::PermissionsExt,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{net::TcpListener, sync::oneshot};
const USER: &str = "agent-probe";
struct Handler {
    key: PublicKey,
    active: Arc<AtomicUsize>,
    connection: bool,
    forwarding: Arc<Forwarding>,
    grants: std::collections::HashSet<ChannelId>,
}
impl Drop for Handler {
    fn drop(&mut self) {
        if self.connection {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl server::Server for Handler {
    type Handler = Self;
    fn new_client(&mut self, _peer: Option<std::net::SocketAddr>) -> Self {
        self.active.fetch_add(1, Ordering::SeqCst);
        Self {
            key: self.key.clone(),
            active: self.active.clone(),
            connection: true,
            forwarding: self.forwarding.clone(),
            grants: std::collections::HashSet::new(),
        }
    }
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_publickey(
        &mut self,
        user: &str,
        key: &PublicKey,
    ) -> std::result::Result<server::Auth, Self::Error> {
        Ok(if user == USER && key == &self.key {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
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
    async fn agent_request(
        &mut self,
        channel: ChannelId,
        _session: &mut server::Session,
    ) -> std::result::Result<bool, Self::Error> {
        self.grants.insert(channel);
        Ok(true)
    }
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if data == b"agent-forwarded-fixed-exec" {
            if !self.grants.contains(&channel) {
                self.forwarding.denied.fetch_add(1, Ordering::SeqCst);
                session.channel_failure(channel)?;
                return Ok(());
            }
            let Ok(permit) = self.forwarding.slots.clone().try_acquire_owned() else {
                session.channel_failure(channel)?;
                return Ok(());
            };
            session.channel_success(channel)?;
            let handle = session.handle();
            let key = self.key.clone();
            let state = self.forwarding.clone();
            state.active.fetch_add(1, Ordering::SeqCst);
            let guard = ForwardGuard(state.clone());
            tokio::spawn(async move {
                let _permit = permit;
                let _guard = guard;
                let job = async {
                    let remote = handle.channel_open_agent().await?;
                    let mut agent = AgentClient::connect(remote.into_stream());
                    let identities = agent.request_identities().await?;
                    ensure!(
                        identities.len() == 1
                            && matches!(&identities[0],AgentIdentity::PublicKey{key:listed,..}if listed==&key),
                        "forwarded Agent identity differs"
                    );
                    let message = b"fixed forwarded signing test";
                    let signature = agent.sign_request_signature(&key, None, message).await?;
                    russh::keys::signature::Verifier::verify(&key, message, &signature)?;
                    drop(agent);
                    handle
                        .data(channel, b"FORWARDED_OK\n".to_vec())
                        .await
                        .map_err(|_| anyhow::anyhow!("forwarded output failed"))?;
                    handle
                        .exit_status_request(channel, 7)
                        .await
                        .map_err(|_| anyhow::anyhow!("forwarded exit failed"))?;
                    handle
                        .eof(channel)
                        .await
                        .map_err(|_| anyhow::anyhow!("forwarded EOF failed"))?;
                    handle
                        .close(channel)
                        .await
                        .map_err(|_| anyhow::anyhow!("forwarded close failed"))?;
                    Ok::<_, anyhow::Error>(())
                };
                if matches!(
                    tokio::time::timeout(Duration::from_secs(3), job).await,
                    Ok(Ok(()))
                ) {
                    state.completed.fetch_add(1, Ordering::SeqCst);
                } else {
                    let _ = handle.exit_status_request(channel, 1).await;
                    let _ = handle.eof(channel).await;
                    let _ = handle.close(channel).await;
                }
            });
            return Ok(());
        }

        if data != b"agent-fixed-exec" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        session.data(channel, b"AGENT_OK\n".to_vec())?;
        session.exit_status_request(channel, 7)?;
        session.eof(channel)?;
        session.close(channel)?;
        Ok(())
    }
}
struct Forwarding {
    active: AtomicUsize,
    completed: AtomicUsize,
    denied: AtomicUsize,
    slots: Arc<tokio::sync::Semaphore>,
}
impl Default for Forwarding {
    fn default() -> Self {
        Self {
            active: AtomicUsize::new(0),
            completed: AtomicUsize::new(0),
            denied: AtomicUsize::new(0),
            slots: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }
}
struct ForwardGuard(Arc<Forwarding>);
impl Drop for ForwardGuard {
    fn drop(&mut self) {
        self.0.active.fetch_sub(1, Ordering::SeqCst);
    }
}
async fn forwarded_system_client(
    address: std::net::SocketAddr,
    key: &PublicKey,
    identity: &std::path::Path,
    socket: &std::path::Path,
    dir: &std::path::Path,
) -> Result<()> {
    use tokio::io::AsyncReadExt;
    let known = dir.join("known_hosts");
    let config = dir.join("empty_config");
    std::fs::write(
        &known,
        format!("[127.0.0.1]:{} {}\n", address.port(), key.to_openssh()?),
    )?;
    std::fs::write(&config, "")?;
    for enabled in [false, true] {
        let mut cmd = tokio::process::Command::new("ssh");
        cmd.arg("-F")
            .arg(&config)
            .args([
                "-T",
                if enabled { "-A" } else { "-a" },
                "-o",
                "BatchMode=yes",
                "-o",
                "IdentitiesOnly=yes",
                "-o",
                "StrictHostKeyChecking=yes",
                "-o",
                "ConnectTimeout=3",
                "-o",
            ])
            .arg(format!("IdentityAgent={}", socket.display()))
            .arg("-o")
            .arg(format!("UserKnownHostsFile={}", known.display()))
            .arg("-o")
            .arg(format!("GlobalKnownHostsFile={}", config.display()))
            .arg("-i")
            .arg(identity)
            .arg("-p")
            .arg(address.port().to_string())
            .arg(format!("{USER}@127.0.0.1"))
            .arg("agent-forwarded-fixed-exec")
            .env("SSH_AUTH_SOCK", socket)
            .env_remove("SSH_AGENT_PID")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
        let mut child = cmd.spawn()?;
        let stdout = child.stdout.take().context("forward SSH stdout missing")?;
        let read = tokio::spawn(async move {
            let mut data = Vec::new();
            stdout.take(1024).read_to_end(&mut data).await.map(|_| data)
        });
        let status = match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
            Ok(status) => status?,
            Err(_) => {
                child.start_kill()?;
                child.wait().await?;
                read.abort();
                anyhow::bail!("forwarding SSH deadline");
            }
        };
        let output = tokio::time::timeout(Duration::from_secs(1), read).await???;
        if enabled {
            ensure!(
                status.code() == Some(7) && output == b"FORWARDED_OK\n",
                "forwarded Agent sign/exit mismatch"
            );
        } else {
            ensure!(
                status.code().is_some_and(|code| code != 0 && code != 7) && output.is_empty(),
                "Agent forward request silently enabled"
            );
        }
    }
    Ok(())
}

struct Guard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for Guard {
    fn drop(&mut self) {
        self.stop.shutdown("test done".into());
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
async fn probe() -> Result<serde_json::Value> {
    let dir = tempfile::tempdir()?;
    let socket = dir.path().join("agent.sock");
    let secret = dir.path().join("identity");
    let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let public = key.public_key().clone();
    std::fs::write(
        &secret,
        key.to_openssh(russh::keys::ssh_key::LineEnding::LF)?
            .as_bytes(),
    )?;
    std::fs::set_permissions(&secret, std::fs::Permissions::from_mode(0o600))?;
    let mut agent_daemon = tokio::process::Command::new("ssh-agent")
        .args(["-D", "-a"])
        .arg(&socket)
        .env_remove("SSH_AUTH_SOCK")
        .env_remove("SSH_AGENT_PID")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .context("isolated software ssh-agent required")?;
    let result=async{
  tokio::time::timeout(Duration::from_secs(3),async{while !socket.exists(){ensure!(agent_daemon.try_wait()?.is_none(),"private agent exited during startup");tokio::time::sleep(Duration::from_millis(10)).await;}Ok::<_,anyhow::Error>(())}).await??;
  let mut agent=tokio::time::timeout(Duration::from_secs(3),AgentClient::connect_uds(&socket)).await??;ensure!(agent.request_identities().await.context("private agent identity request")?.is_empty(),"private agent did not start empty");
  let mut add=tokio::process::Command::new("ssh-add").args(["-q","-t","30"]).arg(&secret).env("SSH_AUTH_SOCK",&socket).env_remove("SSH_AGENT_PID").env_remove("SSH_ASKPASS").env("SSH_ASKPASS_REQUIRE","never").stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).kill_on_drop(true).spawn()?;
  let loaded=tokio::time::timeout(Duration::from_secs(3),add.wait()).await.context("synthetic ssh-add deadline")??;ensure!(loaded.success(),"synthetic agent key load failed");
  let identities=agent.request_identities().await.context("loaded private agent identities")?;ensure!(identities.len()==1&&matches!(&identities[0],AgentIdentity::PublicKey{key,..}if key==&public),"private agent identity mismatch");
  let message=b"fixed synthetic signature\x00UTF8";let signature=agent.sign_request_signature(&public,None,message).await.context("raw private agent signing")?;russh::keys::signature::Verifier::verify(&public,message,&signature)?;
  let host=PrivateKey::random(&mut rand::rng(),Algorithm::Ed25519)?;let pinned=host.public_key().clone();let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await?;let address=listener.local_addr()?;let active=Arc::new(AtomicUsize::new(0));let forwarding=Arc::new(Forwarding::default());let mut handler=Handler{key:public.clone(),active:active.clone(),connection:false,forwarding:forwarding.clone(),grants:std::collections::HashSet::new()};let cfg=Arc::new(server::Config{keys:vec![host],auth_rejection_time:Duration::from_millis(20),auth_rejection_time_initial:Some(Duration::ZERO),inactivity_timeout:Some(Duration::from_secs(5)),..Default::default()});let(tx,rx)=oneshot::channel();let task=tokio::spawn(async move{let running=handler.run_on_socket(cfg,&listener);let _=tx.send(running.handle());running.await});let mut guard=Guard{stop:rx.await?,task:Some(task)};
  let mut session=super::ssh_probe::connect(address,pinned.clone(),Arc::new(AtomicBool::new(false))).await?;
  ensure!(tokio::time::timeout(Duration::from_secs(3),session.authenticate_publickey_with(USER,public.clone(),None,&mut agent)).await?.context("SSH authentication through private agent signer")?.success(),"agent signer auth failed");
  let mut channel=session.channel_open_session().await?;channel.exec(true,b"agent-fixed-exec".to_vec()).await?;ensure!(super::ssh_probe::success(&mut channel).await?,"agent-auth fixed exec denied");let mut output=Vec::new();let mut exit=None;let mut eof=false;let mut close=false;tokio::time::timeout(Duration::from_secs(3),async{while let Some(message)=channel.wait().await{match message{ChannelMsg::Data{data}=>{ensure!(output.len()+data.len()<=1024,"agent channel output bound");output.extend_from_slice(&data);},ChannelMsg::ExitStatus{exit_status}=>exit=Some(exit_status),ChannelMsg::Eof=>eof=true,ChannelMsg::Close=>{close=true;break;},_=>{}}}Ok::<_,anyhow::Error>(())}).await??;ensure!(output==b"AGENT_OK\n"&&exit==Some(7)&&eof&&close,"agent channel tail/EOF missing");
  session.disconnect(Disconnect::ByApplication,"","en").await?;drop(session);tokio::time::timeout(Duration::from_secs(3),async{while active.load(Ordering::SeqCst)!=0{tokio::time::sleep(Duration::from_millis(5)).await;}}).await.context("agent authenticated connection did not drain")?;
  forwarded_system_client(address,&pinned,&secret,&socket,dir.path()).await?;
  tokio::time::timeout(Duration::from_secs(3),async{while active.load(Ordering::SeqCst)!=0||forwarding.active.load(Ordering::SeqCst)!=0{tokio::time::sleep(Duration::from_millis(5)).await;}}).await.context("forwarded Agent session/task did not drain")?;
  ensure!(forwarding.completed.load(Ordering::SeqCst)==1&&forwarding.denied.load(Ordering::SeqCst)==1,"forwarded signature or disabled-client policy refusal proof absent");
  agent.remove_identity(&public).await.context("private agent scoped identity removal")?;ensure!(agent.request_identities().await.context("private agent identity request")?.is_empty(),"synthetic identity remove failed");ensure!(matches!(agent.sign_request_signature(&public,None,message).await,Err(russh::keys::Error::AgentProtocolError)),"removed key refusal was not a protocol denial");ensure!(agent.request_identities().await?.is_empty(),"agent died after removed-key denial");drop(agent);
  guard.stop.shutdown("test complete".into());if let Some(task)=&mut guard.task{tokio::time::timeout(Duration::from_secs(3),task).await???;}guard.task=None;
  Ok::<_,anyhow::Error>(json!({"schemaVersion":1,"privateAgentStartedEmpty":true,"explicitPrivateSocketOnly":true,"singleSyntheticEd25519Identity":true,"agentSignatureVerified":true,"sshAuthenticatedViaAgentSigner":true,"removedIdentityRefusesSign":true,"fixedExecOnly":true,"exitStatus":7,"eofAndClose":true,"connectionsDrainedBeforeListenerStop":true,"listenerJoined":true,"noRealFido2Hardware":true,"forwarding":{"explicitChannelGrantRequired":true,"disabledClientRefused":true,"forwardedSignatureVerified":true,"singleForwardTaskCredit":1,"forwardTaskDrained":true},"limitations":["Private software OpenSSH agent, not hardware FIDO2 or user's ambient identities","Controlled OpenSSH-client-to-russh-server Agent forwarding only; no PulseTerm client forwarding UI/default permission, Pageant/Windows pipe/macOS Agent matrix or hardware confirmation policy"],"platform":std::env::consts::OS}))
 }.await;
    let killed = agent_daemon.kill().await;
    let joined = tokio::time::timeout(Duration::from_secs(3), agent_daemon.wait()).await;
    killed.context("private agent kill failed")?;
    joined.context("private agent join deadline")??;
    let mut report = result?;
    report["agentDaemonJoined"] = json!(true);
    Ok(report)
}
fn validate_report(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["exitStatus"] == 7,
        "agent version/exit evidence"
    );
    for field in [
        "privateAgentStartedEmpty",
        "explicitPrivateSocketOnly",
        "singleSyntheticEd25519Identity",
        "agentSignatureVerified",
        "sshAuthenticatedViaAgentSigner",
        "removedIdentityRefusesSign",
        "fixedExecOnly",
        "eofAndClose",
        "connectionsDrainedBeforeListenerStop",
        "listenerJoined",
        "noRealFido2Hardware",
        "agentDaemonJoined",
    ] {
        ensure!(r[field] == true, "missing Agent evidence: {field}");
    }
    ensure!(
        r["forwarding"]["singleForwardTaskCredit"] == 1,
        "missing forward task credit"
    );
    for field in [
        "explicitChannelGrantRequired",
        "disabledClientRefused",
        "forwardedSignatureVerified",
        "forwardTaskDrained",
    ] {
        ensure!(
            r["forwarding"][field] == true,
            "missing forwarded Agent observation"
        );
    }
    ensure!(
        r["limitations"].as_array().is_some_and(|a| !a.is_empty()),
        "missing agent limitations"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    let file = root.join("target/probes/ssh-agent.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if file.exists() {
        std::fs::remove_file(&file)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(25), probe()).await })??;
    validate_report(&report)?;
    std::fs::write(file, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS private software Agent signing/SSH auth/removal/EOF/cleanup");
    Ok(())
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[tokio::test]
    async fn scoped_identity_removal_emits_valid_frame_not_the_broken_remove_all_header() {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let public = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
            .unwrap()
            .public_key()
            .clone();
        let (client, mut peer) = tokio::io::duplex(1024);
        let mut agent = AgentClient::connect(client);
        let read = async {
            let mut header = [0; 4];
            peer.read_exact(&mut header).await.unwrap();
            let len = u32::from_be_bytes(header) as usize;
            assert!((5..=1024).contains(&len));
            let mut body = vec![0; len];
            peer.read_exact(&mut body).await.unwrap();
            assert_eq!(body[0], 18);
            assert_eq!(
                u32::from_be_bytes(body[1..5].try_into().unwrap()) as usize,
                len - 5
            );
            peer.write_all(&[0, 0, 0, 1, 6]).await.unwrap();
        };
        let result = tokio::time::timeout(Duration::from_secs(1), async {
            tokio::join!(agent.remove_identity(&public), read)
        })
        .await
        .unwrap();
        assert!(result.0.is_ok());
    }
    #[test]
    fn forwarded_agent_uses_single_task_credit_until_its_guard_releases() {
        let state = Arc::new(Forwarding::default());
        state.active.fetch_add(1, Ordering::SeqCst);
        let guard = ForwardGuard(state.clone());
        let permit = state.slots.clone().try_acquire_owned().unwrap();
        assert!(state.slots.clone().try_acquire_owned().is_err());
        assert_eq!(state.active.load(Ordering::SeqCst), 1);
        drop(guard);
        drop(permit);
        assert_eq!(state.active.load(Ordering::SeqCst), 0);
        assert!(state.slots.clone().try_acquire_owned().is_ok());
    }

    #[test]
    fn agent_report_cannot_pass_without_isolation_and_hardware_limit() {
        assert!(
            validate_report(&json!({"schemaVersion":1,"exitStatus":7,"completed":true})).is_err()
        );
    }
}
