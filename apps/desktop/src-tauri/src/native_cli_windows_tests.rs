//! Native Windows fixtures only: no system daemon, GUI, external network or arbitrary pipe input.
use super::*;
use pulse_app::cli_control::{Handler, Operation};
use pulse_ipc::cli::Request;
use serde_json::json;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;
use tokio_util::sync::CancellationToken;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct Echo(AtomicUsize);
impl Handler for Echo {
    fn call(&self, _: Request, _: CancellationToken) -> Operation {
        self.0.fetch_add(1, Ordering::SeqCst);
        Box::pin(async { Ok(json!({"focused":true})) })
    }
}
#[tokio::test]
async fn authenticated_native_pipe_calls_once_and_shutdown_releases_namespace() -> TestResult {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("control");
    let handler = Arc::new(Echo(AtomicUsize::new(0)));
    let server = super::start(&directory, &Handle::current(), handler.clone())?;
    assert!(matches!(
        pulse_platform::windows::local_control_presence(&directory),
        Ok(pulse_platform::windows::LocalControlPresence::Present)
    ));
    let mut client = super::connect(&directory).await?;
    assert_eq!(
        handler.0.load(Ordering::SeqCst),
        0,
        "hello must not execute a handler"
    );
    assert_eq!(
        client.invoke("workspace.focus", json!({})).await?,
        json!({"focused":true})
    );
    assert_eq!(handler.0.load(Ordering::SeqCst), 1);
    drop(client);
    server.shutdown().await?;
    assert!(!directory.join("control.token").exists());
    assert!(matches!(
        pulse_platform::windows::local_control_presence(&directory),
        Ok(pulse_platform::windows::LocalControlPresence::Absent)
    ));
    let restarted = super::start(&directory, &Handle::current(), handler)?;
    restarted.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn wrong_token_cannot_reach_handler_and_stale_token_is_never_repaired() -> TestResult {
    let root = tempfile::tempdir()?;
    let directory = root.path().join("control");
    let handler = Arc::new(Echo(AtomicUsize::new(0)));
    let server = super::start(&directory, &Handle::current(), handler.clone())?;
    assert!(super::start(&directory, &Handle::current(), handler.clone()).is_err());
    std::fs::write(directory.join("control.token"), [b'a'; 64])?;
    assert!(super::connect(&directory).await.is_err());
    assert_eq!(handler.0.load(Ordering::SeqCst), 0);
    server.shutdown().await?;
    let dir = pulse_platform::windows::PrivateDirectory::open(&directory)?;
    drop(dir.create_file("control.token", &[b'b'; 64], 64)?);
    assert!(super::start(&directory, &Handle::current(), handler).is_err());
    assert_eq!(std::fs::read(directory.join("control.token"))?, [b'b'; 64]);
    Ok(())
}
struct Gate {
    entered: Arc<Notify>,
    cancelled: Arc<Notify>,
    release: Arc<Notify>,
}
impl Handler for Gate {
    fn call(&self, _: Request, cancelled: CancellationToken) -> Operation {
        self.entered.notify_one();
        let observed = self.cancelled.clone();
        let release = self.release.clone();
        Box::pin(async move {
            cancelled.cancelled().await;
            observed.notify_one();
            release.notified().await;
            Err(Error)
        })
    }
}
#[tokio::test]
async fn cancelled_shutdown_waiter_keeps_native_operation_and_token_until_completion() -> TestResult
{
    let root = tempfile::tempdir()?;
    let directory = root.path().join("control");
    let handler = Arc::new(Gate {
        entered: Arc::new(Notify::new()),
        cancelled: Arc::new(Notify::new()),
        release: Arc::new(Notify::new()),
    });
    let server = super::start(&directory, &Handle::current(), handler.clone())?;
    let client_directory = directory.clone();
    let client = tokio::spawn(async move {
        let mut client = super::connect(&client_directory).await?;
        client.invoke("workspace.focus", json!({})).await
    });
    tokio::time::timeout(Duration::from_secs(5), handler.entered.notified()).await?;
    server.close();
    tokio::time::timeout(Duration::from_secs(5), handler.cancelled.notified()).await?;
    assert!(
        tokio::time::timeout(Duration::from_millis(30), server.shutdown())
            .await
            .is_err()
    );
    assert!(directory.join("control.token").exists());
    handler.release.notify_one();
    tokio::time::timeout(Duration::from_secs(5), server.shutdown()).await??;
    assert!(client.await?.is_err());
    assert!(!directory.join("control.token").exists());
    Ok(())
}
