use super::*;
use pulse_ssh::{HostPin, HostTarget};
use russh::{
    keys::{PrivateKey, ssh_key::private::Ed25519Keypair},
    server::{self, Server as _},
};
use std::{borrow::Cow, sync::atomic::AtomicUsize, time::Duration};
#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Accept,
    Reject,
    TooMany,
    Oversize,
    ChangedSource,
    Cancel,
    WrongPin,
}
#[derive(Clone)]
struct Server {
    mode: Mode,
    step: usize,
    received: Arc<AtomicUsize>,
    fallback: Arc<AtomicUsize>,
}
impl server::Server for Server {
    type Handler = Self;
    fn new_client(&mut self, _: Option<std::net::SocketAddr>) -> Self {
        self.clone()
    }
}
impl server::Handler for Server {
    type Error = russh::Error;
    async fn auth_password(
        &mut self,
        _: &str,
        _: &str,
    ) -> std::result::Result<server::Auth, Self::Error> {
        self.fallback.fetch_add(1, Ordering::SeqCst);
        Ok(server::Auth::reject())
    }
    async fn auth_keyboard_interactive<'a>(
        &'a mut self,
        user: &str,
        _: &str,
        response: Option<server::Response<'a>>,
    ) -> std::result::Result<server::Auth, Self::Error> {
        if user != "fixture-user" {
            return Ok(server::Auth::reject());
        }
        if let Some(mut answers) = response {
            let exact = answers
                .next()
                .is_some_and(|value| value.as_ref() == b"fixture-answer")
                && answers.next().is_none();
            if !exact {
                return Ok(server::Auth::reject());
            }
            self.received.fetch_add(1, Ordering::SeqCst);
            self.step += 1;
            if self.mode == Mode::Reject {
                return Ok(server::Auth::reject());
            }
            if self.mode != Mode::TooMany && self.step == 2 {
                return Ok(server::Auth::Accept);
            }
        }
        Ok(server::Auth::Partial {
            name: Cow::Borrowed("Test server"),
            instructions: Cow::Borrowed("Test-only challenges"),
            prompts: Cow::Owned(vec![(
                Cow::Owned(if self.mode == Mode::Oversize {
                    "X".repeat(1025)
                } else {
                    format!("<literal> Test round {}", self.step + 1)
                }),
                self.step == 0,
            )]),
        })
    }
}
struct ServerGuard {
    stop: server::RunningServerHandle,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}
impl Drop for ServerGuard {
    fn drop(&mut self) {
        self.stop.shutdown("fixture end".into());
        if let Some(task) = &self.task {
            task.abort();
        }
    }
}
async fn run(mode: Mode) -> TestResult {
    let key = PrivateKey::new(Ed25519Keypair::from_seed(&[7; 32]).into(), "")?;
    let public = key.public_key().clone();
    let config = Arc::new(server::Config {
        keys: vec![key],
        auth_rejection_time: Duration::ZERO,
        auth_rejection_time_initial: Some(Duration::ZERO),
        ..Default::default()
    });
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
    let address = listener.local_addr()?;
    let received = Arc::new(AtomicUsize::new(0));
    let fallback = Arc::new(AtomicUsize::new(0));
    let mut server = Server {
        mode,
        step: 0,
        received: received.clone(),
        fallback: fallback.clone(),
    };
    let (tx, rx) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let running = server.run_on_socket(config, &listener);
        let _ = tx.send(running.handle());
        running.await
    });
    let mut server_guard = ServerGuard {
        stop: rx.await?,
        task: Some(task),
    };
    let target = HostTarget::new("127.0.0.1", address.port())?;
    let pin = if mode == Mode::WrongPin {
        PrivateKey::new(Ed25519Keypair::from_seed(&[8; 32]).into(), "")?
            .public_key()
            .clone()
    } else {
        public
    };
    let pending = PinnedTransport::begin_connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target, pin)]),
        Some(vec![]),
        Arc::new(russh::client::Config::default()),
        tokio::net::TcpStream::connect(address).await?,
    )?;
    let native = pending.native_io_completion();
    let connected = tokio::time::timeout(Duration::from_secs(3), pending.connect()).await?;
    if mode == Mode::WrongPin {
        assert!(connected.is_err());
        tokio::time::timeout(Duration::from_secs(2), native.wait()).await??;
        assert_eq!(received.load(Ordering::SeqCst), 0);
        assert_eq!(fallback.load(Ordering::SeqCst), 0);
    } else {
        let mut transport = connected?;
        let peer = transport.peer()?;
        let (broker, mut packets) = channel();
        let b = broker.clone();
        let live = Arc::new(AtomicBool::new(true));
        let change = live.clone();
        let emitted = Arc::new(AtomicUsize::new(0));
        let count = emitted.clone();
        let (cancel, cancelled) = oneshot::channel();
        let answerer = tokio::spawn(async move {
            let mut cancel = Some(cancel);
            while let Some(packet) = packets.recv().await {
                let index = count.fetch_add(1, Ordering::SeqCst);
                assert_eq!(packet["prompts"][0]["echo"], index == 0);
                let round = Uuid::parse_str(packet["round"].as_str().ok_or("fixture.round")?)?;
                if mode == Mode::Cancel {
                    if let Some(cancel) = cancel.take() {
                        let _ = cancel.send(());
                    }
                    break;
                }
                if mode == Mode::ChangedSource {
                    change.store(false, Ordering::Release);
                }
                b.answer(round, Zeroizing::new(vec!["fixture-answer".into()]))?;
            }
            std::result::Result::<(), Box<dyn std::error::Error + Send + Sync>>::Ok(())
        });
        let authorization = live.clone();
        let operation = authenticate_client(
            &mut transport,
            &peer,
            &broker,
            "fixture-user",
            Arc::new(|| true),
            move || {
                std::future::ready(if authorization.load(Ordering::Acquire) {
                    Ok(())
                } else {
                    Err(VaultOwnerError::Denied)
                })
            },
        );
        let outcome = tokio::time::timeout(Duration::from_secs(3), async {
            tokio::select! {
                result=operation=>Some(result),_ = cancelled=>None,
            }
        })
        .await?;
        match mode {
            Mode::Accept => {
                assert!(matches!(outcome, Some(Ok(true))));
                assert_eq!(received.load(Ordering::SeqCst), 2);
            }
            Mode::Reject => {
                assert!(matches!(outcome, Some(Ok(false))));
                assert_eq!(received.load(Ordering::SeqCst), 1);
            }
            Mode::TooMany => {
                assert!(matches!(outcome, Some(Err(_))));
                assert_eq!(emitted.load(Ordering::SeqCst), 8);
            }
            Mode::Oversize | Mode::ChangedSource => {
                assert!(matches!(outcome, Some(Err(_))));
                assert_eq!(received.load(Ordering::SeqCst), 0);
            }
            Mode::Cancel => {
                assert!(outcome.is_none());
                assert!(broker.pending.lock().map_err(|_| "poison")?.is_none());
            }
            Mode::WrongPin => return Err("fixture.unreachable".into()),
        }
        answerer.abort();
        let _ = answerer.await;
        tokio::time::timeout(Duration::from_secs(2), transport.close_and_join()).await??;
        tokio::time::timeout(Duration::from_secs(2), native.wait()).await??;
        assert_eq!(fallback.load(Ordering::SeqCst), 0);
    }
    server_guard.stop.shutdown("fixture end".into());
    if let Some(task) = server_guard.task.take() {
        tokio::time::timeout(Duration::from_secs(2), task).await???;
    }
    Ok(())
}
#[tokio::test]
async fn real_peer_accepts_two_explicit_rounds_and_never_falls_back() -> TestResult {
    run(Mode::Accept).await?;
    run(Mode::Reject).await
}
#[tokio::test]
async fn real_peer_challenge_limits_and_wrong_pin_fail_closed() -> TestResult {
    run(Mode::TooMany).await?;
    run(Mode::Oversize).await?;
    run(Mode::WrongPin).await
}
#[tokio::test]
async fn real_peer_cancel_and_changed_source_send_no_answers_and_join_native_io() -> TestResult {
    run(Mode::Cancel).await?;
    run(Mode::ChangedSource).await
}
