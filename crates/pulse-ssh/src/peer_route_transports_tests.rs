use super::*;
use std::{
    pin::Pin,
    task::{Context, Poll},
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

struct GateIo {
    stream: tokio::net::TcpStream,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl AsyncRead for GateIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_read(cx, buf)
    }
}
impl AsyncWrite for GateIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Pin::new(&mut self.stream).poll_write(cx, buf)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.stream).poll_shutdown(cx)
    }
}
impl Drop for GateIo {
    fn drop(&mut self) {
        let _sent = self.entered.send(());
        let _released = self.release.recv_timeout(Duration::from_secs(5));
    }
}
struct Fixture {
    listener: ListenerGuard,
    address: std::net::SocketAddr,
    public: russh::keys::PublicKey,
    auth: Arc<AtomicUsize>,
}
impl Fixture {
    async fn new() -> Result<Self, Box<dyn std::error::Error>> {
        let secret = key()?;
        let public = secret.public_key().clone();
        let cfg = Arc::new(server::Config {
            keys: vec![secret],
            auth_rejection_time: Duration::ZERO,
            auth_rejection_time_initial: Some(Duration::ZERO),
            ..Default::default()
        });
        let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0)).await?;
        let address = listener.local_addr()?;
        let auth = Arc::new(AtomicUsize::new(0));
        let mut spy = Spy {
            auth: auth.clone(),
            active: Arc::new(AtomicUsize::new(0)),
            child: false,
        };
        let (sender, reply) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let running = spy.run_on_socket(cfg, &listener);
            let _sent = sender.send(running.handle());
            running.await
        });
        Ok(Self {
            listener: ListenerGuard {
                stop: reply.await?,
                task: Some(task),
            },
            address,
            public,
            auth,
        })
    }
    async fn connect<R: AsyncRead + AsyncWrite + Unpin + Send + 'static>(
        &self,
        stream: R,
    ) -> Result<PinnedTransport, PeerError> {
        let target =
            HostTarget::new("127.0.0.1", self.address.port()).map_err(PeerError::HostKey)?;
        PinnedTransport::connect_stream(
            target.clone(),
            Some(vec![HostPin::new(target, self.public.clone())]),
            Some(vec![]),
            Arc::new(client::Config::default()),
            stream,
        )
        .await
    }
    async fn close(mut self) -> TestResult {
        assert_eq!(
            self.auth.load(Ordering::SeqCst),
            0,
            "ownership must never authenticate"
        );
        self.listener.stop.shutdown("fixed test cleanup".into());
        if let Some(task) = self.listener.task.take() {
            task.await??;
        }
        Ok(())
    }
}

#[tokio::test]
async fn bounded_collection_refuses_without_losing_native_resources_and_final_is_single_use()
-> TestResult {
    let runtime = tokio::runtime::Handle::current();
    assert!(RouteTransports::new(0, runtime.clone()).is_err());
    assert!(RouteTransports::new(10, runtime.clone()).is_err());
    RouteTransports::new(9, runtime.clone())?
        .begin_close()
        .wait()
        .await?;
    let fixture = Fixture::new().await?;
    let first = fixture
        .connect(tokio::net::TcpStream::connect(fixture.address).await?)
        .await?;
    let extra = fixture
        .connect(tokio::net::TcpStream::connect(fixture.address).await?)
        .await?;
    let extra_io = extra.native_io_completion();
    let mut owner = RouteTransports::new(1, runtime)?;
    assert!(owner.take_final().is_err());
    assert!(owner.push(first).is_ok());
    let (error, extra) = match owner.push(extra) {
        Err(rejected) => rejected.into_parts(),
        Ok(()) => return Err("over-limit transport admitted".into()),
    };
    assert!(matches!(error, PeerError::Transport));
    assert!(extra.peer()?.is_current());
    assert!(!extra_io.is_complete());
    let final_transport = owner.take_final()?;
    assert!(owner.take_final().is_err());
    let (_, extra) = match owner.push(extra) {
        Err(rejected) => rejected.into_parts(),
        Ok(()) => return Err("transport inserted after final transfer".into()),
    };
    extra.close_and_join().await?;
    extra_io.wait().await?;
    final_transport.close_and_join().await?;
    owner.begin_close().wait().await?;
    fixture.close().await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 3)]
async fn cancelled_cleanup_waiter_still_drains_inside_out_and_keeps_admissions_until_native_exit()
-> TestResult {
    let fixture = Fixture::new().await?;
    let mut outer = fixture
        .connect(tokio::net::TcpStream::connect(fixture.address).await?)
        .await?;
    let outer_peer = outer.peer()?;
    let outer_admission = Arc::new(());
    let outer_weak = Arc::downgrade(&outer_admission);
    let outer_io = outer.retain_native_admission(&outer_peer, outer_admission)?;
    let (entered, received) = std::sync::mpsc::channel();
    let (release, gate) = std::sync::mpsc::channel();
    let mut inner = fixture
        .connect(GateIo {
            stream: tokio::net::TcpStream::connect(fixture.address).await?,
            entered,
            release: gate,
        })
        .await?;
    let inner_peer = inner.peer()?;
    let inner_admission = Arc::new(());
    let inner_weak = Arc::downgrade(&inner_admission);
    let inner_io = inner.retain_native_admission(&inner_peer, inner_admission)?;
    let mut owner = RouteTransports::new(2, tokio::runtime::Handle::current())?;
    assert!(owner.push(outer).is_ok());
    assert!(owner.push(inner).is_ok());
    drop(owner.begin_close());
    tokio::task::spawn_blocking(move || received.recv_timeout(Duration::from_secs(3))).await??;
    let ordering = outer_peer.is_current() && !inner_peer.is_current();
    let retained = !inner_io.is_complete()
        && !outer_io.is_complete()
        && inner_weak.upgrade().is_some()
        && outer_weak.upgrade().is_some();
    release.send(())?;
    tokio::time::timeout(Duration::from_secs(3), inner_io.wait()).await??;
    tokio::time::timeout(Duration::from_secs(3), outer_io.wait()).await??;
    assert!(ordering, "outer close waits for inner SDK and stream exit");
    assert!(
        retained,
        "cancelled result waiter cannot release native credit"
    );
    assert!(inner_weak.upgrade().is_none() && outer_weak.upgrade().is_none());
    assert!(!outer_peer.is_current());
    fixture.close().await
}
