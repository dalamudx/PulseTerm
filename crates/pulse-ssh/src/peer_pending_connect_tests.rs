use super::*;
use std::{
    io,
    pin::Pin,
    task::{Context, Poll, Waker},
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, ReadBuf};
struct HeldDrop<R> {
    inner: R,
    entered: std::sync::mpsc::Sender<()>,
    release: std::sync::mpsc::Receiver<()>,
}
impl<R> Drop for HeldDrop<R> {
    fn drop(&mut self) {
        let _ = self.entered.send(());
        let _ = self.release.recv();
    }
}
impl<R: AsyncRead + Unpin> AsyncRead for HeldDrop<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_read(cx, buf)
    }
}
impl<R: AsyncWrite + Unpin> AsyncWrite for HeldDrop<R> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_shutdown(cx)
    }
}
#[tokio::test]
async fn cold_attempt_drop_and_cancelled_observer_wait_for_underlying_destructor() -> TestResult {
    let target = HostTarget::new("fixed.invalid", 22)?;
    let key = key()?;
    let (stream, _remote) = tokio::io::duplex(2048);
    let (entered, received) = std::sync::mpsc::channel();
    let (release, held) = std::sync::mpsc::channel();
    let attempt = PinnedTransport::begin_connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target, key.public_key().clone())]),
        None,
        Arc::new(client::Config::default()),
        HeldDrop {
            inner: stream,
            entered,
            release: held,
        },
    )?;
    let done = attempt.native_io_completion();
    let mut abandoned = Box::pin(attempt.native_io_completion().wait());
    let thread = std::thread::spawn(move || drop(attempt));
    tokio::task::spawn_blocking(move || received.recv()).await??;
    assert!(!done.is_complete());
    assert!(matches!(
        std::future::Future::poll(abandoned.as_mut(), &mut Context::from_waker(Waker::noop())),
        Poll::Pending
    ));
    drop(abandoned);
    assert!(!done.is_complete());
    release.send(())?;
    tokio::time::timeout(std::time::Duration::from_secs(2), done.wait()).await??;
    assert!(thread.join().is_ok());
    Ok(())
}
#[tokio::test]
async fn spawned_sdk_handshake_cancel_proves_actual_tcp_eof_before_native_release() -> TestResult {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    let target = HostTarget::new("127.0.0.1", addr.port())?;
    let key = key()?;
    let tcp = tokio::net::TcpStream::connect(addr).await?;
    let attempt = PinnedTransport::begin_connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target, key.public_key().clone())]),
        None,
        Arc::new(client::Config::default()),
        tcp,
    )?;
    let done = attempt.native_io_completion();
    let task = tokio::spawn(attempt.connect());
    let (remote, _) = listener.accept().await?;
    let mut remote = tokio::io::BufReader::new(remote);
    let mut identification = Vec::new();
    assert!(
        tokio::time::timeout(
            std::time::Duration::from_secs(2),
            remote.read_until(b'\n', &mut identification)
        )
        .await??
            > 0
    );
    remote
        .get_mut()
        .write_all(b"SSH-2.0-PulseTerm-fixture\r\n")
        .await?;
    let mut kex = [0u8; 6];
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        remote.read_exact(&mut kex),
    )
    .await??;
    assert_eq!(kex[5], 20); // SSH_MSG_KEXINIT emitted by session.run AFTER SDK spawn.
    assert!(!done.is_complete());
    task.abort();
    assert!(task.await.is_err_and(|e| e.is_cancelled()));
    tokio::time::timeout(std::time::Duration::from_secs(2), done.wait()).await??;
    let mut rest = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        remote.read_to_end(&mut rest),
    )
    .await??;
    Ok(())
}
#[tokio::test]
async fn original_five_second_timeout_still_drains_the_owned_stream() -> TestResult {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0)).await?;
    let addr = listener.local_addr()?;
    let target = HostTarget::new("127.0.0.1", addr.port())?;
    let key = key()?;
    let attempt = PinnedTransport::begin_connect_stream(
        target.clone(),
        Some(vec![HostPin::new(target, key.public_key().clone())]),
        None,
        Arc::new(client::Config::default()),
        tokio::net::TcpStream::connect(addr).await?,
    )?;
    let done = attempt.native_io_completion();
    let task = tokio::spawn(attempt.connect());
    let (remote, _) = listener.accept().await?;
    let mut remote = tokio::io::BufReader::new(remote);
    let mut identification = Vec::new();
    remote.read_until(b'\n', &mut identification).await?;
    remote
        .get_mut()
        .write_all(b"SSH-2.0-PulseTerm-fixture\r\n")
        .await?;
    let mut kex = [0u8; 6];
    remote.read_exact(&mut kex).await?;
    assert_eq!(kex[5], 20);
    let outcome = tokio::time::timeout(std::time::Duration::from_secs(6), task).await??;
    assert!(matches!(outcome, Err(PeerError::Transport)));
    tokio::time::timeout(std::time::Duration::from_secs(2), done.wait()).await??;
    let mut rest = Vec::new();
    tokio::time::timeout(
        std::time::Duration::from_secs(2),
        remote.read_to_end(&mut rest),
    )
    .await??;
    Ok(())
}
