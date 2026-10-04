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
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
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
  let host=PrivateKey::random(&mut rand::rng(),Algorithm::Ed25519)?;let pinned=host.public_key().clone();let listener=TcpListener::bind((std::net::Ipv4Addr::LOCALHOST,0)).await?;let address=listener.local_addr()?;let active=Arc::new(AtomicUsize::new(0));let mut handler=Handler{key:public.clone(),active:active.clone(),connection:false};let cfg=Arc::new(server::Config{keys:vec![host],auth_rejection_time:Duration::from_millis(20),auth_rejection_time_initial:Some(Duration::ZERO),inactivity_timeout:Some(Duration::from_secs(5)),..Default::default()});let(tx,rx)=oneshot::channel();let task=tokio::spawn(async move{let running=handler.run_on_socket(cfg,&listener);let _=tx.send(running.handle());running.await});let mut guard=Guard{stop:rx.await?,task:Some(task)};
  let mut session=super::ssh_probe::connect(address,pinned,Arc::new(AtomicBool::new(false))).await?;
  ensure!(tokio::time::timeout(Duration::from_secs(3),session.authenticate_publickey_with(USER,public.clone(),None,&mut agent)).await?.context("SSH authentication through private agent signer")?.success(),"agent signer auth failed");
  let mut channel=session.channel_open_session().await?;channel.exec(true,b"agent-fixed-exec".to_vec()).await?;ensure!(super::ssh_probe::success(&mut channel).await?,"agent-auth fixed exec denied");let mut output=Vec::new();let mut exit=None;let mut eof=false;let mut close=false;tokio::time::timeout(Duration::from_secs(3),async{while let Some(message)=channel.wait().await{match message{ChannelMsg::Data{data}=>{ensure!(output.len()+data.len()<=1024,"agent channel output bound");output.extend_from_slice(&data);},ChannelMsg::ExitStatus{exit_status}=>exit=Some(exit_status),ChannelMsg::Eof=>eof=true,ChannelMsg::Close=>{close=true;break;},_=>{}}}Ok::<_,anyhow::Error>(())}).await??;ensure!(output==b"AGENT_OK\n"&&exit==Some(7)&&eof&&close,"agent channel tail/EOF missing");
  session.disconnect(Disconnect::ByApplication,"","en").await?;drop(session);tokio::time::timeout(Duration::from_secs(3),async{while active.load(Ordering::SeqCst)!=0{tokio::time::sleep(Duration::from_millis(5)).await;}}).await.context("agent authenticated connection did not drain")?;
  agent.remove_identity(&public).await.context("private agent scoped identity removal")?;ensure!(agent.request_identities().await.context("private agent identity request")?.is_empty(),"synthetic identity remove failed");ensure!(matches!(agent.sign_request_signature(&public,None,message).await,Err(russh::keys::Error::AgentProtocolError)),"removed key refusal was not a protocol denial");ensure!(agent.request_identities().await?.is_empty(),"agent died after removed-key denial");drop(agent);
  guard.stop.shutdown("test complete".into());if let Some(task)=&mut guard.task{tokio::time::timeout(Duration::from_secs(3),task).await???;}guard.task=None;
  Ok::<_,anyhow::Error>(json!({"schemaVersion":1,"privateAgentStartedEmpty":true,"explicitPrivateSocketOnly":true,"singleSyntheticEd25519Identity":true,"agentSignatureVerified":true,"sshAuthenticatedViaAgentSigner":true,"removedIdentityRefusesSign":true,"fixedExecOnly":true,"exitStatus":7,"eofAndClose":true,"connectionsDrainedBeforeListenerStop":true,"listenerJoined":true,"noRealFido2Hardware":true,"limitations":["Private software OpenSSH agent, not hardware FIDO2 or user's ambient identities","No remote agent forwarding, Pageant/Windows pipe/macOS Agent matrix, confirmation UX or production secret-vault integration"],"platform":std::env::consts::OS}))
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
    fn agent_report_cannot_pass_without_isolation_and_hardware_limit() {
        assert!(
            validate_report(&json!({"schemaVersion":1,"exitStatus":7,"completed":true})).is_err()
        );
    }
}
