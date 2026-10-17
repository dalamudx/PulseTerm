use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn original_handshake_timeout_wakes_actual_stream_without_caller_abort() -> TestResult {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let (stream, mut observer) = tokio::io::duplex(4096);
    let target = HostTarget::new("fixed.example", 22)?;
    let task = tokio::spawn(PinnedTransport::connect_stream(
        target,
        Some(vec![]),
        Some(vec![]),
        Arc::new(client::Config::default()),
        stream,
    ));
    let mut buffer = [0; 4096];
    assert!(observer.read(&mut buffer).await? > 0);
    observer
        .write_all(b"SSH-2.0-fixed-timeout-fixture\r\n")
        .await?;
    assert!(observer.read(&mut buffer).await? > 0);
    tokio::time::sleep(std::time::Duration::from_secs(5)).await;
    assert!(matches!(task.await?, Err(PeerError::Transport)));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut drained = 0;
        loop {
            let count = observer.read(&mut buffer).await?;
            if count == 0 {
                return Ok::<(), std::io::Error>(());
            }
            drained += count;
            if drained > 8192 {
                return Err(std::io::Error::other("fixture.output_budget"));
            }
        }
    })
    .await??;
    Ok(())
}
struct ReentrantWake {
    shared: Arc<Shared>,
    count: std::sync::atomic::AtomicUsize,
}
impl std::task::Wake for ReentrantWake {
    fn wake(self: Arc<Self>) {
        assert!(self.shared.state.try_lock().is_ok());
        assert!(self.shared.wakeups.try_lock().is_ok());
        self.count.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn close_wakes_pending_read_and_write_outside_both_short_locks() -> TestResult {
    use std::{
        pin::Pin,
        task::{Context, Poll, Waker},
    };
    use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};
    let state = Arc::new(Shared {
        io: native_io::IoLifetime::new(),
        wakeups: Mutex::new([None, None]),
        closed: AtomicBool::new(false),
        state: Mutex::new(State {
            claim: None,
            refusal: None,
        }),
    });
    let wake = Arc::new(ReentrantWake {
        shared: state.clone(),
        count: std::sync::atomic::AtomicUsize::new(0),
    });
    let waker = Waker::from(wake.clone());
    let mut cx = Context::from_waker(&waker);
    let (stream, _observer) = tokio::io::duplex(1);
    let mut stream = revocable_io::RevocableIo::new(stream, state.clone());
    let mut bytes = [0; 1];
    let mut read = ReadBuf::new(&mut bytes);
    assert!(
        Pin::new(&mut stream)
            .poll_read(&mut cx, &mut read)
            .is_pending()
    );
    assert!(matches!(
        Pin::new(&mut stream).poll_write(&mut cx, b"a"),
        Poll::Ready(Ok(1))
    ));
    assert!(Pin::new(&mut stream).poll_write(&mut cx, b"b").is_pending());
    state.close();
    assert_eq!(wake.count.load(Ordering::SeqCst), 2);
    assert!(matches!(
        Pin::new(&mut stream).poll_read(&mut cx, &mut read),
        Poll::Ready(Err(_))
    ));
    assert!(matches!(
        Pin::new(&mut stream).poll_write(&mut cx, b"c"),
        Poll::Ready(Err(_))
    ));
    Ok(())
}
