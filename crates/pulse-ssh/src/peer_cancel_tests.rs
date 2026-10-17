use super::*;
type TestResult = Result<(), Box<dyn std::error::Error>>;
#[tokio::test]
async fn abort_after_session_started_drops_owned_stream_without_detached_handshake() -> TestResult {
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
    let initial = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        observer.read(&mut buffer),
    )
    .await
    .map_err(|_| "fixture.initial_id_timeout")??;
    assert!(initial > 0);
    observer
        .write_all(b"SSH-2.0-fixed-cancel-fixture\r\n")
        .await?;
    let packet = tokio::time::timeout(
        std::time::Duration::from_secs(2),
        observer.read(&mut buffer),
    )
    .await
    .map_err(|_| "fixture.kex_timeout")??;
    assert!(packet > 0);
    task.abort();
    assert!(task.await.is_err_and(|error| error.is_cancelled()));
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        let mut drained = 0;
        loop {
            let size = observer.read(&mut buffer).await?;
            if size == 0 {
                return Ok::<(), std::io::Error>(());
            }
            drained += size;
            if drained > 8192 {
                return Err(std::io::Error::other("fixture.output_budget"));
            }
        }
    })
    .await
    .map_err(|_| "fixture.cancelled_stream_eof_timeout")??;
    Ok(())
}
