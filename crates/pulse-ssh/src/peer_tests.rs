use super::*;
use russh::client::Handler as _;
use russh::{
    keys::{Algorithm, PrivateKey},
    server::{self, Server},
};
use std::sync::atomic::AtomicUsize;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[path = "peer_pending_connect_tests.rs"]
mod pending_connect_tests;
#[path = "peer_route_transports_tests.rs"]
mod route_transports_tests;
fn key() -> Result<PrivateKey, Box<dyn std::error::Error>> {
    Ok(PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?)
}
fn shared() -> Arc<Shared> {
    Arc::new(Shared {
        io: native_io::IoLifetime::new(),
        wakeups: Mutex::new([None, None]),
        closed: AtomicBool::new(false),
        state: Mutex::new(State {
            claim: None,
            refusal: None,
        }),
    })
}
fn proof(shared: &Arc<Shared>) -> Result<VerifiedPeer, PeerError> {
    Ok(VerifiedPeer {
        shared: Arc::downgrade(shared),
        claim: shared.guard()?.claim.clone().ok_or(PeerError::Stale)?,
    })
}
#[tokio::test]
async fn callback_rekey_drop_and_poison_never_turn_pins_into_independent_permission() -> TestResult
{
    let target = HostTarget::new("fixed.example", 22)?;
    let first = key()?;
    let second = key()?;
    let state = shared();
    let guard = CloseGuard(state.clone());
    let mut handler = PeerHandler {
        target: target.clone(),
        own: vec![
            HostPin::new(target.clone(), first.public_key().clone()),
            HostPin::new(target.clone(), second.public_key().clone()),
        ],
        system: None,
        shared: state.clone(),
    };
    assert!(proof(&state).is_err());
    assert!(
        handler
            .check_server_key(&first.public_key().clone().into())
            .await?
    );
    let original = proof(&state)?;
    assert!(original.is_current());
    assert!(
        handler
            .check_server_key(&first.public_key().clone().into())
            .await?
    );
    assert!(original.is_current());
    assert!(
        handler
            .check_server_key(&second.public_key().clone().into())
            .await?
    );
    assert!(!original.is_current());
    let newer = proof(&state)?;
    assert!(newer.is_current());
    drop(guard);
    assert!(!newer.is_current());
    drop(handler);
    let state = shared();
    state.guard()?.claim = Some(Arc::new(Claim {
        target,
        key: first.public_key().clone(),
    }));
    let value = proof(&state)?;
    let poisoned = state.clone();
    let result = std::panic::catch_unwind(move || {
        let _held = poisoned.state.lock().unwrap_or_else(|p| p.into_inner());
        panic!("fixed-test-poison")
    });
    assert!(result.is_err());
    assert!(!value.is_current());
    assert!(state.closed.load(Ordering::Acquire));
    Ok(())
}
#[path = "peer_observe_tests.rs"]
mod observe_tests;

#[cfg(all(feature = "managed-store", unix))]
#[path = "first_pin_tests.rs"]
mod first_pin_tests;

struct Spy {
    auth: Arc<AtomicUsize>,
    active: Arc<AtomicUsize>,
    child: bool,
}
impl Drop for Spy {
    fn drop(&mut self) {
        if self.child {
            self.active.fetch_sub(1, Ordering::SeqCst);
        }
    }
}
impl Server for Spy {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        self.active.fetch_add(1, Ordering::SeqCst);
        Self {
            auth: self.auth.clone(),
            active: self.active.clone(),
            child: true,
        }
    }
}
impl server::Handler for Spy {
    type Error = russh::Error;
    async fn auth_none(&mut self, _: &str) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_password(
        &mut self,
        user: &str,
        password: &str,
    ) -> Result<server::Auth, Self::Error> {
        self.auth.fetch_add(1, Ordering::SeqCst);
        Ok(if user == "fixed" && password == "fixed-test-only" {
            server::Auth::Accept
        } else {
            server::Auth::reject()
        })
    }
}
struct ListenerGuard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for ListenerGuard {
    fn drop(&mut self) {
        self.stop.shutdown("fixed test end".into());
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
#[tokio::test]
async fn real_tcp_wrong_key_pre_auth_and_same_key_connection_source_are_distinct() -> TestResult {
    let secret = key()?;
    let good = secret.public_key().clone();
    let bad = key()?.public_key().clone();
    let cfg = Arc::new(server::Config {
        keys: vec![secret],
        auth_rejection_time: std::time::Duration::ZERO,
        auth_rejection_time_initial: Some(std::time::Duration::ZERO),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let auth = Arc::new(AtomicUsize::new(0));
    let active = Arc::new(AtomicUsize::new(0));
    let mut spy = Spy {
        auth: auth.clone(),
        active: active.clone(),
        child: false,
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let running = spy.run_on_socket(cfg, &listener);
        let _sent = tx.send(running.handle());
        running.await
    });
    let mut guard = ListenerGuard {
        stop: rx.await?,
        task: Some(task),
    };
    let target = HostTarget::new("127.0.0.1", address.port())?;
    let wrong_attempt = PinnedTransport::begin_connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target.clone(), bad)]),
        Some(vec![]),
        Arc::new(client::Config::default()),
        tokio::net::TcpStream::connect(address).await?,
    )?;
    let refused_io = wrong_attempt.native_io_completion();
    let wrong = wrong_attempt.connect().await;
    tokio::time::timeout(std::time::Duration::from_secs(2), refused_io.wait()).await??;
    assert!(matches!(
        wrong,
        Err(PeerError::HostKey(HostKeyError::Changed))
    ));
    assert_eq!(auth.load(Ordering::SeqCst), 0);
    let mut first = PinnedTransport::connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target.clone(), good.clone())]),
        None,
        Arc::new(client::Config::default()),
        tokio::net::TcpStream::connect(address).await?,
    )
    .await?;
    let peer = first.peer()?;
    assert!(peer.is_current());
    assert_eq!(peer.target(), &target);
    assert_eq!(auth.load(Ordering::SeqCst), 0);
    let mut second = PinnedTransport::connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target, good)]),
        None,
        Arc::new(client::Config::default()),
        tokio::net::TcpStream::connect(address).await?,
    )
    .await?;
    assert!(matches!(second.client(&peer), Err(PeerError::Stale)));
    assert!(matches!(
        second.retain_native_admission(&peer, Arc::new(())),
        Err(PeerError::Stale)
    ));
    let reservation = Arc::new(());
    let held = Arc::downgrade(&reservation);
    let completion = first.retain_native_admission(&peer, reservation)?;
    assert!(!completion.is_complete());
    assert!(held.upgrade().is_some());
    assert!(matches!(
        first.retain_native_admission(&peer, Arc::new(())),
        Err(PeerError::AdmissionBusy)
    ));
    assert!(peer.is_current());
    // Explicit fixed caller authentication is a test, not a Vault/target permission implied by peer evidence.
    assert!(
        first
            .client(&peer)?
            .authenticate_password("fixed", "fixed-test-only")
            .await?
            .success()
    );
    assert_eq!(auth.load(Ordering::SeqCst), 1);
    tokio::time::timeout(std::time::Duration::from_secs(2), first.close_and_join()).await??;
    assert!(!peer.is_current());
    tokio::time::timeout(std::time::Duration::from_secs(2), completion.wait()).await??;
    assert!(held.upgrade().is_none());
    let other = second.peer()?;
    let reservation = Arc::new(());
    let held_other = Arc::downgrade(&reservation);
    let other_completion = second.retain_native_admission(&other, reservation)?;
    assert!(other.is_current());
    guard.stop.shutdown("fixed test complete".into());
    if let Some(task) = guard.task.take() {
        tokio::time::timeout(std::time::Duration::from_secs(2), task).await???;
    }
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while active.load(Ordering::SeqCst) != 0 {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while other.is_current() {
            tokio::task::yield_now().await;
        }
    })
    .await?;
    tokio::time::timeout(std::time::Duration::from_secs(2), other_completion.wait()).await??;
    assert!(held_other.upgrade().is_none());
    assert!(second.peer().is_err());
    assert!(matches!(second.client(&other), Err(PeerError::Stale)));
    tokio::time::timeout(std::time::Duration::from_secs(2), second.close_and_join()).await??;
    Ok(())
}
#[tokio::test]
async fn absent_trust_source_before_handshake_does_not_write_to_owned_stream() -> TestResult {
    use tokio::io::AsyncReadExt;
    let (stream, mut observer) = tokio::io::duplex(64);
    let target = HostTarget::new("fixed.example", 22)?;
    assert!(matches!(
        PinnedTransport::connect_stream(
            target,
            None,
            Some(vec![]),
            Arc::new(client::Config::default()),
            stream
        )
        .await,
        Err(PeerError::HostKey(HostKeyError::StoreUnavailable))
    ));
    let mut byte = [0];
    assert_eq!(observer.read(&mut byte).await?, 0);
    Ok(())
}
