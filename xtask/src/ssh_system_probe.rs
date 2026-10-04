//! Real OpenSSH server test, private config/keys/loopback; no user SSH config edits.
use anyhow::{Context, Result, ensure};
use russh::{
    Channel, ChannelMsg, Disconnect, client,
    keys::{Algorithm, PrivateKey, PrivateKeyWithHashAlg, PublicKey},
};
use serde_json::json;
use std::{
    io::{BufRead, Write},
    path::Path,
    process::Stdio,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tokio::net::{TcpListener, TcpStream};
/// Fixed ForceCommand target. Client-supplied shell text is NEVER executed.
pub(crate) fn helper() -> Result<()> {
    let original = std::env::var("SSH_ORIGINAL_COMMAND").unwrap_or_default();
    ensure!(
        original.is_empty(),
        "fixed helper only permits shell request"
    );
    std::thread::spawn(|| {
        std::thread::sleep(Duration::from_secs(12));
        std::process::exit(2);
    });
    let mut out = std::io::stdout().lock();
    writeln!(out, "SYSTEM_READY")?;
    out.flush()?;
    for line in std::io::stdin().lock().lines() {
        let line = line?;
        ensure!(line.len() <= 512, "fixed helper input bound");
        match line.as_str() {
            "size" => {
                let size = std::process::Command::new("stty")
                    .arg("size")
                    .stdin(Stdio::inherit())
                    .output()?;
                ensure!(
                    size.status.success() && size.stdout.len() < 64,
                    "PTY dimensions unavailable"
                );
                writeln!(
                    out,
                    "SYSTEM_SIZE:{}",
                    std::str::from_utf8(&size.stdout)?.trim()
                )?;
            }
            "literal ; $(not-executed) 中文🌍" => {
                writeln!(out, "SYSTEM_ECHO:{line}")?;
            }
            "exit" => {
                writeln!(out, "SYSTEM_END")?;
                out.flush()?;
                std::process::exit(7);
            }
            _ => anyhow::bail!("unexpected fixed helper input"),
        };
        out.flush()?;
    }
    anyhow::bail!("fixed helper unexpected input EOF")
}
fn private_write(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::write(path, bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
    }
    Ok(())
}
async fn marker(channel: &mut Channel<client::Msg>, expected: &[u8]) -> Result<()> {
    let mut received = Vec::new();
    tokio::time::timeout(Duration::from_secs(3), async {
        loop {
            match channel.wait().await {
                Some(ChannelMsg::Data { data }) => {
                    ensure!(received.len() + data.len() <= 16384, "OpenSSH output bound");
                    received.extend_from_slice(&data);
                    if received
                        .windows(expected.len())
                        .any(|window| window == expected)
                    {
                        return Ok::<_, anyhow::Error>(());
                    }
                }
                Some(ChannelMsg::Close) | None => anyhow::bail!("SSH closed before marker"),
                _ => {}
            }
        }
    })
    .await??;
    Ok(())
}
fn legacy_preferred() -> russh::Preferred {
    use std::borrow::Cow;
    russh::Preferred {
        kex: Cow::Owned(vec![russh::kex::DH_G14_SHA1]),
        cipher: Cow::Owned(vec![russh::cipher::AES_128_CTR]),
        mac: Cow::Owned(vec![russh::mac::HMAC_SHA1]),
        key: Cow::Owned(vec![Algorithm::Ed25519]),
        ..Default::default()
    }
}
async fn profile_connect(
    address: std::net::SocketAddr,
    key: PublicKey,
    mismatch: Arc<AtomicBool>,
    legacy: bool,
) -> Result<russh::client::Handle<super::ssh_probe::PinnedClient>> {
    let preferred = if legacy {
        legacy_preferred()
    } else {
        russh::Preferred::default()
    };
    super::ssh_probe::connect_with_preferred(address, key, mismatch, preferred).await
}
async fn secure_reject(address: std::net::SocketAddr, key: PublicKey) -> Result<()> {
    let mismatch = Arc::new(AtomicBool::new(false));
    match super::ssh_probe::connect(address, key, mismatch.clone()).await {
        Err(error) => ensure!(
            matches!(
                error.downcast_ref::<russh::Error>(),
                Some(russh::Error::NoCommonAlgo {
                    kind: russh::AlgorithmKind::Kex,
                    ..
                })
            ) && !mismatch.load(Ordering::SeqCst),
            "secure default failed for non-KEX reason"
        ),
        Ok(_) => anyhow::bail!("secure default implicitly enabled legacy KEX"),
    };
    Ok(())
}

async fn probe(legacy: bool) -> Result<serde_json::Value> {
    ensure!(
        cfg!(target_os = "linux"),
        "real OpenSSH PTY probe currently requires Linux/stty"
    );
    ensure!(
        Path::new("/run/sshd").is_dir(),
        "existing OpenSSH privilege separation directory required"
    );
    let version = tokio::process::Command::new("/usr/sbin/sshd")
        .arg("-V")
        .output()
        .await
        .context("OpenSSH server required")?;
    ensure!(version.stderr.len() < 1024, "sshd version output bound");
    let version = String::from_utf8(version.stderr)?
        .split_whitespace()
        .next()
        .context("sshd version missing")?
        .to_owned();
    ensure!(version.starts_with("OpenSSH_"), "not OpenSSH server");
    let uid = tokio::process::Command::new("id")
        .arg("-un")
        .output()
        .await?;
    ensure!(
        uid.status.success() && uid.stdout.len() < 64,
        "current test account unavailable"
    );
    let user = std::str::from_utf8(&uid.stdout)?.trim();
    ensure!(
        !user.is_empty()
            && user
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-'),
        "unsupported local test account"
    );
    let dir = tempfile::tempdir()?;
    let host = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let identity = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let host_path = dir.path().join("host_key");
    let authorized = dir.path().join("authorized_keys");
    let config = dir.path().join("sshd_config");
    let log = dir.path().join("sshd.log");
    private_write(
        &host_path,
        host.to_openssh(russh::keys::ssh_key::LineEnding::LF)?
            .as_bytes(),
    )?;
    private_write(&authorized, identity.public_key().to_openssh()?.as_bytes())?;
    let reservation = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = reservation.local_addr()?;
    drop(reservation);
    let exe = std::env::current_exe()?;
    let command = format!(
        "'{}' ssh-system-helper",
        exe.to_string_lossy().replace('\'', "'\\''")
    );
    let text = format!(
        "ListenAddress 127.0.0.1\nPort {}\nHostKey {}\nPidFile {}\nAuthorizedKeysFile {}\nAllowUsers {}\nAuthenticationMethods publickey\nPasswordAuthentication no\nKbdInteractiveAuthentication no\nPermitRootLogin prohibit-password\nStrictModes no\nUsePAM yes\nAllowAgentForwarding no\nAllowTcpForwarding no\nX11Forwarding no\nPermitTunnel no\nPermitUserEnvironment no\nPrintMotd no\nPrintLastLog no\nForceCommand {}\nLogLevel ERROR\n",
        address.port(),
        host_path.display(),
        dir.path().join("pid").display(),
        authorized.display(),
        user,
        command
    );
    let text = if legacy {
        format!(
            "{text}KexAlgorithms diffie-hellman-group14-sha1\nCiphers aes128-ctr\nMACs hmac-sha1\n"
        )
    } else {
        text
    };
    private_write(&config, text.as_bytes())?;
    let logfile = std::fs::File::create(&log)?;
    let mut child = tokio::process::Command::new("/usr/sbin/sshd")
        .args(["-D", "-e", "-f"])
        .arg(&config)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(logfile)
        .kill_on_drop(true)
        .spawn()?;
    let daemon_pid = child.id().context("sshd PID unavailable")?;
    let result=async{
  tokio::time::timeout(Duration::from_secs(3),async{loop{ensure!(child.try_wait()?.is_none(),"isolated sshd exited during startup");if TcpStream::connect(address).await.is_ok(){break;}tokio::time::sleep(Duration::from_millis(20)).await;}Ok::<_,anyhow::Error>(())}).await??;
  if legacy{secure_reject(address,host.public_key().clone()).await?;}
  let wrong=PrivateKey::random(&mut rand::rng(),Algorithm::Ed25519)?.public_key().clone();let rejected=Arc::new(AtomicBool::new(false));ensure!(profile_connect(address,wrong,rejected.clone(),legacy).await.is_err()&&rejected.load(Ordering::SeqCst),"real host-key mismatch not rejected");
  let mut session=profile_connect(address,host.public_key().clone(),Arc::new(AtomicBool::new(false)),legacy).await?;
  ensure!(session.authenticate_publickey(user,PrivateKeyWithHashAlg::new(Arc::new(identity),None)).await?.success(),"real sshd publickey denied");
  let mut channel=session.channel_open_session().await?;channel.request_pty(true,"xterm-256color",80,24,0,0,&[]).await?;ensure!(super::ssh_probe::success(&mut channel).await?,"real PTY request denied");channel.request_shell(true).await?;ensure!(super::ssh_probe::success(&mut channel).await?,"fixed ForceCommand shell denied");marker(&mut channel,b"SYSTEM_READY").await?;
  channel.data(&b"size\n"[..]).await?;marker(&mut channel,b"SYSTEM_SIZE:24 80").await?;
  channel.window_change(91,37,0,0).await?;channel.data(&b"size\n"[..]).await?;marker(&mut channel,b"SYSTEM_SIZE:37 91").await?;
  channel.data("literal ; $(not-executed) 中文🌍\n".as_bytes()).await?;marker(&mut channel,"SYSTEM_ECHO:literal ; $(not-executed) 中文🌍".as_bytes()).await?;
  channel.data(&b"exit\n"[..]).await?;let mut output=Vec::new();let mut code=None;let mut eof=false;tokio::time::timeout(Duration::from_secs(3),async{while let Some(message)=channel.wait().await{match message{ChannelMsg::Data{data}=>{ensure!(output.len()+data.len()<=1024,"real EOF output bound");output.extend_from_slice(&data);},ChannelMsg::ExitStatus{exit_status}=>code=Some(exit_status),ChannelMsg::Eof=>eof=true,ChannelMsg::Close=>break,_=>{}}}Ok::<_,anyhow::Error>(())}).await??;
  ensure!(code==Some(7)&&eof&&output.windows(10).any(|w|w==b"SYSTEM_END"),"real PTY EOF/status/output missing");session.disconnect(Disconnect::ByApplication,"","en").await?;
  if legacy{secure_reject(address,host.public_key().clone()).await?;}
  wait_children(daemon_pid).await?;
  Ok::<_,anyhow::Error>(json!({"schemaVersion":1,"serverVersion":version,"loopbackOnly":true,"privateConfigAndKeys":true,"changedHostKeyRejected":true,"publicKeyAuth":true,"realNativePty":true,"initialNativeSize":[80,24],"resizedNativeSize":[91,37],"utf8LiteralInput":true,"fixedForceCommand":true,"nativeSessionsDrained":true,"exitStatus":7,"eofOutputDrained":true,"limitations":["Current system OpenSSH server, not the OpenSSH7-9 matrix","Existing local test account/PAM used; no account or global/user SSH config modified","StrictModes disabled ONLY in private test daemon config; tempdir private and keys0600","No actor SSH adapter, SFTP, forwarding, Agent/FIDO2, certificates, legacy or GBK validation"],"platform":std::env::consts::OS}))
 }.await;
    let stop = child.kill().await;
    let joined = tokio::time::timeout(Duration::from_secs(3), child.wait()).await;
    stop.context("isolated sshd stop failed")?;
    joined.context("isolated sshd join deadline")??;
    let mut report = result?;
    report["legacyExplicitOptIn"] = json!(legacy);
    report["secureDefaultRejectedLegacy"] = json!(legacy);
    report["nativeDaemonJoined"] = json!(true);
    Ok(report)
}
async fn wait_children(pid: u32) -> Result<()> {
    tokio::time::timeout(Duration::from_secs(3), async {
        let path = format!("/proc/{pid}/task/{pid}/children");
        loop {
            let children =
                std::fs::read_to_string(&path).context("owned sshd children status unavailable")?;
            ensure!(children.len() < 1024, "too many owned sshd children");
            if children.trim().is_empty() {
                return Ok::<_, anyhow::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .context("owned sshd sessions did not drain")??;
    Ok(())
}
fn mode_matches(r: &serde_json::Value, legacy: bool) -> bool {
    r["legacyExplicitOptIn"].as_bool() == Some(legacy)
        && r["secureDefaultRejectedLegacy"].as_bool() == Some(legacy)
}

fn validate_report(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1
            && r["serverVersion"]
                .as_str()
                .is_some_and(|s| s.starts_with("OpenSSH_") && s.len() < 64),
        "system SSH identity/version"
    );
    for field in [
        "loopbackOnly",
        "privateConfigAndKeys",
        "changedHostKeyRejected",
        "publicKeyAuth",
        "realNativePty",
        "utf8LiteralInput",
        "fixedForceCommand",
        "nativeSessionsDrained",
        "eofOutputDrained",
        "nativeDaemonJoined",
    ] {
        ensure!(r[field] == true, "missing system SSH evidence: {field}");
    }
    ensure!(
        r["initialNativeSize"] == json!([80, 24])
            && r["resizedNativeSize"] == json!([91, 37])
            && r["exitStatus"] == 7,
        "system native PTY/exit evidence"
    );
    ensure!(
        r["limitations"].as_array().is_some_and(|a| !a.is_empty()),
        "system SSH limitations missing"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    run_profile(false)
}
pub fn run_legacy() -> Result<()> {
    run_profile(true)
}
fn run_profile(legacy: bool) -> Result<()> {
    let root = super::root()?;
    let destination = root.join(if legacy {
        "target/probes/ssh-system-legacy.json"
    } else {
        "target/probes/ssh-system.json"
    });
    std::fs::create_dir_all(root.join("target/probes"))?;
    if destination.exists() {
        std::fs::remove_file(&destination)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(25), probe(legacy)).await })??;
    validate_report(&report)?;
    ensure!(
        mode_matches(&report, legacy),
        "legacy/default mode mismatch"
    );
    std::fs::write(destination, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS russh to isolated real OpenSSH host-key/publickey/native PTY/window/UTF8/EOF");
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn legacy_opt_in_does_not_mutate_secure_default_kex_or_mac() {
        let before = russh::Preferred::default();
        let legacy = legacy_preferred();
        assert!(legacy.kex.contains(&russh::kex::DH_G14_SHA1));
        assert!(legacy.mac.contains(&russh::mac::HMAC_SHA1));
        assert_eq!(legacy.key.as_ref(), &[Algorithm::Ed25519]);
        let after = russh::Preferred::default();
        assert_eq!(before.kex, after.kex);
        assert!(!after.kex.contains(&russh::kex::DH_G14_SHA1));
        assert!(!after.mac.contains(&russh::mac::HMAC_SHA1));
    }
    #[test]
    fn legacy_report_cannot_claim_modern_profile_or_omit_explicit_opt_in() {
        for enabled in [false, true] {
            let r = json!({"legacyExplicitOptIn":enabled,"secureDefaultRejectedLegacy":enabled});
            assert!(mode_matches(&r, enabled));
            assert!(!mode_matches(&r, !enabled));
        }
        assert!(!mode_matches(&json!({}), false));
        assert!(!mode_matches(
            &json!({"legacyExplicitOptIn":"true","secureDefaultRejectedLegacy":true}),
            true
        ));
    }

    #[test]
    fn real_server_report_requires_native_pty_exit_and_child_drain() {
        let report = json!({"schemaVersion":1,"serverVersion":"OpenSSH_10.0p2","loopbackOnly":true,"privateConfigAndKeys":true,"changedHostKeyRejected":true,"publicKeyAuth":true,"realNativePty":true,"utf8LiteralInput":true,"fixedForceCommand":true,"nativeSessionsDrained":true,"eofOutputDrained":true,"nativeDaemonJoined":true,"initialNativeSize":[80,24],"resizedNativeSize":[91,37],"exitStatus":7,"limitations":["not7-9"]});
        assert!(validate_report(&report).is_ok());
        for field in [
            "realNativePty",
            "nativeSessionsDrained",
            "eofOutputDrained",
            "nativeDaemonJoined",
        ] {
            let mut bad = report.clone();
            bad[field] = json!(false);
            assert!(validate_report(&bad).is_err());
        }
        for (field, value) in [
            ("initialNativeSize", json!([24, 80])),
            ("resizedNativeSize", json!([80, 24])),
            ("exitStatus", json!(0)),
            ("limitations", json!([])),
        ] {
            let mut bad = report.clone();
            bad[field] = value;
            assert!(validate_report(&bad).is_err());
        }
        assert!(validate_report(&json!({"completed":true})).is_err());
    }
}
