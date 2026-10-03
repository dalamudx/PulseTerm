//! Restrictive test-only CA policy. Production CA storage/revocation is not implemented.
use anyhow::{Context, Result, ensure};
use russh::server::Server as _;
use russh::{
    Channel, ChannelId, ChannelMsg, Disconnect,
    keys::{
        Algorithm, Certificate, HashAlg, PrivateKey, PublicKey,
        ssh_key::certificate::{Builder, CertType},
    },
    server,
};
use serde_json::json;
use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::{net::TcpListener, sync::oneshot};
const USER: &str = "certificate-probe";
fn authorized(cert: &Certificate, user: &str, ca: &PublicKey, now: u64) -> bool {
    user == USER
        && cert.cert_type() == CertType::User
        && cert.valid_principals().iter().any(|p| p == USER)
        && cert.critical_options().is_empty()
        && cert
            .validate_at(now, [&ca.fingerprint(HashAlg::Sha256)])
            .is_ok()
}
fn issued(
    leaf: &PrivateKey,
    ca: &PrivateKey,
    after: u64,
    before: u64,
    principal: &str,
    kind: CertType,
    critical: bool,
) -> Result<Certificate> {
    let mut b = Builder::new_with_random_nonce(
        &mut rand::rng(),
        leaf.public_key().key_data().clone(),
        after,
        before,
    )?;
    b.key_id("synthetic-certificate")?;
    b.valid_principal(principal)?;
    b.cert_type(kind)?;
    if critical {
        b.critical_option("force-command", "not-executed")?;
    }
    Ok(b.sign(ca)?)
}
#[derive(Default)]
struct Metrics {
    active: AtomicUsize,
    accepted: AtomicUsize,
    rejected: AtomicUsize,
}
struct Handler {
    ca: PublicKey,
    metrics: Arc<Metrics>,
    connection: bool,
}
impl Drop for Handler {
    fn drop(&mut self) {
        if self.connection {
            self.metrics.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl server::Server for Handler {
    type Handler = Self;
    fn new_client(&mut self, _addr: Option<std::net::SocketAddr>) -> Self {
        self.metrics.active.fetch_add(1, Ordering::SeqCst);
        Self {
            ca: self.ca.clone(),
            metrics: self.metrics.clone(),
            connection: true,
        }
    }
}
impl server::Handler for Handler {
    type Error = russh::Error;
    async fn auth_openssh_certificate(
        &mut self,
        user: &str,
        cert: &Certificate,
    ) -> std::result::Result<server::Auth, Self::Error> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| russh::Error::Disconnect)?
            .as_secs();
        if authorized(cert, user, &self.ca, now) {
            self.metrics.accepted.fetch_add(1, Ordering::SeqCst);
            Ok(server::Auth::Accept)
        } else {
            self.metrics.rejected.fetch_add(1, Ordering::SeqCst);
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
    async fn exec_request(
        &mut self,
        channel: ChannelId,
        data: &[u8],
        session: &mut server::Session,
    ) -> std::result::Result<(), Self::Error> {
        if data != b"certificate-fixed-exec" {
            session.channel_failure(channel)?;
            return Ok(());
        }
        session.channel_success(channel)?;
        session.data(channel, b"CERT_OK\n".to_vec())?;
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
    let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let wrong_ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let leaf = Arc::new(PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?);
    let host = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?;
    let pinned = host.public_key().clone();
    let now = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();
    ensure!(now > 120, "system time invalid");
    let good = issued(&leaf, &ca, now - 60, now + 600, USER, CertType::User, false)?;
    let negatives = [
        (
            "unknown-ca",
            issued(
                &leaf,
                &wrong_ca,
                now - 60,
                now + 600,
                USER,
                CertType::User,
                false,
            )?,
        ),
        (
            "expired",
            issued(&leaf, &ca, now - 120, now - 60, USER, CertType::User, false)?,
        ),
        (
            "not-yet-valid",
            issued(
                &leaf,
                &ca,
                now + 600,
                now + 1200,
                USER,
                CertType::User,
                false,
            )?,
        ),
        (
            "wrong-principal",
            issued(
                &leaf,
                &ca,
                now - 60,
                now + 600,
                "different-user",
                CertType::User,
                false,
            )?,
        ),
        (
            "host-type",
            issued(&leaf, &ca, now - 60, now + 600, USER, CertType::Host, false)?,
        ),
        (
            "critical-option",
            issued(&leaf, &ca, now - 60, now + 600, USER, CertType::User, true)?,
        ),
    ];
    ensure!(
        authorized(&good, USER, ca.public_key(), now),
        "valid certificate policy failed"
    );
    for (_, cert) in &negatives {
        ensure!(
            !authorized(cert, USER, ca.public_key(), now),
            "invalid certificate policy accepted"
        );
    }
    let listener = TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let metrics = Arc::new(Metrics::default());
    let mut handler = Handler {
        ca: ca.public_key().clone(),
        metrics: metrics.clone(),
        connection: false,
    };
    let cfg = Arc::new(server::Config {
        keys: vec![host],
        auth_rejection_time: Duration::from_millis(20),
        auth_rejection_time_initial: Some(Duration::ZERO),
        inactivity_timeout: Some(Duration::from_secs(5)),
        ..Default::default()
    });
    let (tx, rx) = oneshot::channel();
    let task = tokio::spawn(async move {
        let running = handler.run_on_socket(cfg, &listener);
        let _ = tx.send(running.handle());
        running.await
    });
    let mut guard = Guard {
        stop: rx.await?,
        task: Some(task),
    };
    for (label, cert) in negatives {
        let mut session =
            super::ssh_probe::connect(address, pinned.clone(), Arc::new(AtomicBool::new(false)))
                .await?;
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            session.authenticate_openssh_cert(USER, leaf.clone(), cert),
        )
        .await??;
        ensure!(
            !outcome.success(),
            "invalid certificate authenticated: {label}"
        );
        session
            .disconnect(Disconnect::ByApplication, "", "en")
            .await?;
    }
    let mut wrong_user =
        super::ssh_probe::connect(address, pinned.clone(), Arc::new(AtomicBool::new(false)))
            .await?;
    ensure!(
        !wrong_user
            .authenticate_openssh_cert("different-user", leaf.clone(), good.clone())
            .await?
            .success(),
        "certificate username not enforced"
    );
    wrong_user
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    let wrong_leaf = Arc::new(PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?);
    let mut nonowner =
        super::ssh_probe::connect(address, pinned.clone(), Arc::new(AtomicBool::new(false)))
            .await?;
    ensure!(
        !nonowner
            .authenticate_openssh_cert(USER, wrong_leaf, good.clone())
            .await?
            .success(),
        "certificate did not require subject private key possession"
    );
    nonowner
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    ensure!(
        metrics.accepted.load(Ordering::SeqCst) == 0,
        "invalid certificate consumed accepted auth"
    );
    let mut session =
        super::ssh_probe::connect(address, pinned, Arc::new(AtomicBool::new(false))).await?;
    ensure!(
        session
            .authenticate_openssh_cert(USER, leaf, good)
            .await?
            .success(),
        "valid trusted certificate rejected"
    );
    let mut channel = session.channel_open_session().await?;
    channel
        .exec(true, b"certificate-fixed-exec".to_vec())
        .await?;
    ensure!(
        super::ssh_probe::success(&mut channel).await?,
        "certificate exec request rejected"
    );
    let mut output = Vec::new();
    let mut exit = None;
    let mut eof = false;
    let mut closed = false;
    tokio::time::timeout(Duration::from_secs(3), async {
        while let Some(message) = channel.wait().await {
            match message {
                ChannelMsg::Data { data } => {
                    ensure!(output.len() + data.len() <= 1024, "cert output bound");
                    output.extend_from_slice(&data);
                }
                ChannelMsg::ExitStatus { exit_status } => exit = Some(exit_status),
                ChannelMsg::Eof => eof = true,
                ChannelMsg::Close => {
                    closed = true;
                    break;
                }
                _ => {}
            }
        }
        Ok::<_, anyhow::Error>(())
    })
    .await??;
    ensure!(
        output == b"CERT_OK\n" && exit == Some(7) && eof && closed,
        "certificate channel bytes/EOF/exit mismatch"
    );
    session
        .disconnect(Disconnect::ByApplication, "", "en")
        .await?;
    drop(session);
    tokio::time::timeout(Duration::from_secs(3), async {
        while metrics.active.load(Ordering::SeqCst) != 0 {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .context("certificate connections did not drain independently")?;
    ensure!(
        metrics.accepted.load(Ordering::SeqCst) == 1,
        "valid certificate accepted count"
    );
    guard.stop.shutdown("done".into());
    if let Some(task) = &mut guard.task {
        tokio::time::timeout(Duration::from_secs(3), task).await???;
    }
    guard.task = None;
    Ok(
        json!({"schemaVersion":1,"loopbackOnly":true,"trustedCaAndSignatureVerified":true,"explicitUserPrincipalRequired":true,"userCertificateTypeRequired":true,"criticalOptionsRejected":true,"expiredAndFutureRejected":true,"unknownCaRejected":true,"negativeCertificates":6,"wrongUsernameRejected":true,"leafKeyOwnershipRequired":true,"validCertificateAuth":true,"fixedExecOnly":true,"outputDrained":true,"exitStatus":7,"eofAndClose":true,"connectionsDrainedBeforeListenerStop":true,"listenerJoined":true,"limitations":["Controlled temporary CA/client/server policy only, not production CA store, revocation or certificate UI","No hardware Agent/FIDO2, OpenSSH7-9 certificate implementation matrix, PKCS11, key encryption or SSH actor transport"],"platform":std::env::consts::OS}),
    )
}
fn validate_report(r: &serde_json::Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["negativeCertificates"] == 6 && r["exitStatus"] == 7,
        "certificate report version/negative/exit"
    );
    for field in [
        "loopbackOnly",
        "trustedCaAndSignatureVerified",
        "explicitUserPrincipalRequired",
        "userCertificateTypeRequired",
        "criticalOptionsRejected",
        "expiredAndFutureRejected",
        "unknownCaRejected",
        "wrongUsernameRejected",
        "leafKeyOwnershipRequired",
        "validCertificateAuth",
        "fixedExecOnly",
        "outputDrained",
        "eofAndClose",
        "connectionsDrainedBeforeListenerStop",
        "listenerJoined",
    ] {
        ensure!(r[field] == true, "missing cert evidence: {field}");
    }
    ensure!(
        r["limitations"].as_array().is_some_and(|a| !a.is_empty()),
        "missing certificate limitations"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    let file = root.join("target/probes/ssh-certificate.json");
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
    println!("PASS trusted CA/user principal/validity/critical-option SSH certificates");
    Ok(())
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn user_cert_policy_checks_time_ca_type_principal_options_and_username() {
        let ca = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let other = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let leaf = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519).unwrap();
        let good = issued(&leaf, &ca, 100, 200, USER, CertType::User, false).unwrap();
        assert!(authorized(&good, USER, ca.public_key(), 100));
        assert!(authorized(&good, USER, ca.public_key(), 199));
        for time in [99, 200] {
            assert!(!authorized(&good, USER, ca.public_key(), time));
        }
        assert!(!authorized(&good, "wrong", ca.public_key(), 100));
        assert!(!authorized(&good, USER, other.public_key(), 100));
        for cert in [
            issued(&leaf, &ca, 100, 200, "wrong", CertType::User, false).unwrap(),
            issued(&leaf, &ca, 100, 200, USER, CertType::Host, false).unwrap(),
            issued(&leaf, &ca, 100, 200, USER, CertType::User, true).unwrap(),
        ] {
            assert!(!authorized(&cert, USER, ca.public_key(), 100));
        }
        let mut b = Builder::new_with_random_nonce(
            &mut rand::rng(),
            leaf.public_key().key_data().clone(),
            100,
            200,
        )
        .unwrap();
        b.all_principals_valid().unwrap();
        let golden = b.sign(&ca).unwrap();
        assert!(!authorized(&golden, USER, ca.public_key(), 100));
    }
    #[test]
    fn flags_without_cert_auth_principal_and_owner_proof_are_rejected() {
        assert!(
            validate_report(
                &json!({"schemaVersion":1,"negativeCertificates":6,"exitStatus":7,"completed":true})
            )
            .is_err()
        );
    }
}
