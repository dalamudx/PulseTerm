//! Wake and fail pending owned I/O on scope cancellation, including detached russh handshake work.
use super::Shared;
use std::{
    io,
    pin::Pin,
    sync::{Arc, atomic::Ordering},
    task::{Context, Poll},
};
use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
pub(super) struct RevocableIo<R> {
    inner: Option<R>,
    shared: Arc<Shared>,
}
impl<R> RevocableIo<R> {
    pub(super) fn new(inner: R, shared: Arc<Shared>) -> Self {
        Self {
            inner: Some(inner),
            shared,
        }
    }
    fn register(&self, cx: &Context<'_>, writer: bool) -> io::Result<()> {
        let next = cx.waker().clone();
        let old = {
            let mut slots = match self.shared.wakeups.lock() {
                Ok(slots) => slots,
                Err(poison) => {
                    drop(poison);
                    self.shared.close();
                    return Err(closed());
                }
            };
            if self.shared.closed.load(Ordering::Acquire) {
                return Err(closed());
            }
            slots[usize::from(writer)].replace(next)
        };
        drop(old);
        Ok(())
    }
}
impl<R> Drop for RevocableIo<R> {
    fn drop(&mut self) {
        self.shared.close();
        let admission = self.shared.io.begin_drop();
        drop(self.inner.take());
        drop(admission);
        self.shared.io.finished();
    }
}
fn closed() -> io::Error {
    io::Error::new(io::ErrorKind::ConnectionAborted, "ssh.peer.closed")
}
impl<R: AsyncRead + Unpin> AsyncRead for RevocableIo<R> {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.register(cx, false) {
            return Poll::Ready(Err(error));
        }
        Pin::new(match this.inner.as_mut() {
            Some(inner) => inner,
            None => return Poll::Ready(Err(closed())),
        })
        .poll_read(cx, buffer)
    }
}
impl<R: AsyncWrite + Unpin> AsyncWrite for RevocableIo<R> {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        let this = self.get_mut();
        if let Err(error) = this.register(cx, true) {
            return Poll::Ready(Err(error));
        }
        Pin::new(match this.inner.as_mut() {
            Some(inner) => inner,
            None => return Poll::Ready(Err(closed())),
        })
        .poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.register(cx, true) {
            return Poll::Ready(Err(error));
        }
        Pin::new(match this.inner.as_mut() {
            Some(inner) => inner,
            None => return Poll::Ready(Err(closed())),
        })
        .poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.get_mut();
        if let Err(error) = this.register(cx, true) {
            return Poll::Ready(Err(error));
        }
        Pin::new(match this.inner.as_mut() {
            Some(inner) => inner,
            None => return Poll::Ready(Err(closed())),
        })
        .poll_shutdown(cx)
    }
}
