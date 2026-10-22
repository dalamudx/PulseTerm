use super::*;
use interprocess::local_socket::GenericFilePath;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::{
    os::unix::fs::PermissionsExt,
    sync::atomic::{AtomicUsize, Ordering},
};
use tokio::sync::Notify;
type TestResult = std::result::Result<(), Box<dyn std::error::Error>>;
struct TestHandler {
    calls: Arc<AtomicUsize>,
    entered: Arc<Notify>,
    cancelled: Arc<Notify>,
    released: Arc<Notify>,
    stall: bool,
}
impl Handler for TestHandler {
    fn call(&self, request: Request, cancelled: CancellationToken) -> Operation {
        let calls = self.calls.clone();
        let entered = self.entered.clone();
        let observed = self.cancelled.clone();
        let released = self.released.clone();
        let stall = self.stall;
        Box::pin(async move {
            if request.method() != "workspace.focus" || !request.params().is_empty() {
                return Err(Error);
            }
            calls.fetch_add(1, Ordering::SeqCst);
            entered.notify_one();
            if stall {
                cancelled.cancelled().await;
                observed.notify_one();
                released.notified().await;
            }
            if cancelled.is_cancelled() {
                return Err(Error);
            }
            Ok(json!({"focusRequested":true}))
        })
    }
}
fn handler(stall: bool) -> Arc<TestHandler> {
    Arc::new(TestHandler {
        calls: Arc::new(AtomicUsize::new(0)),
        entered: Arc::new(Notify::new()),
        cancelled: Arc::new(Notify::new()),
        released: Arc::new(Notify::new()),
        stall,
    })
}
fn directory() -> std::io::Result<tempfile::TempDir> {
    let directory = tempfile::Builder::new()
        .prefix("cli-control-")
        .tempdir_in("/data/temp")?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    Ok(directory)
}
async fn raw(directory: &Path) -> Result<Stream> {
    IpcStream::connect(
        directory
            .join("control.sock")
            .to_fs_name::<GenericFilePath>()
            .map_err(|_| Error)?,
    )
    .await
    .map(Stream)
    .map_err(|_| Error)
}
async fn hello(stream: &Stream, directory: &Path) -> Result<()> {
    let token =
        Zeroizing::new(fs::read_to_string(directory.join("control.token")).map_err(|_| Error)?);
    let mut value = json!({"jsonrpc":"2.0","id":"hello","method":"hello","params":{"protocolVersion":1,"token":token.as_str()}});
    let wire = cli::encode(&value).map_err(|_| Error)?;
    if let Value::String(token) = &mut value["params"]["token"] {
        token.zeroize();
    }
    let mut writer = stream;
    writer.write_all(&wire).await.map_err(|_| Error)?;
    let _ = frame(stream, None).await?;
    Ok(())
}
#[tokio::test]
async fn actual_private_auth_known_action_and_unknown_args_denied() -> TestResult {
    let directory = directory()?;
    let handler = handler(false);
    let server = Server::start(directory.path(), &Handle::current(), handler.clone())?;
    assert_eq!(
        invoke(directory.path(), "workspace.focus", json!({})).await?,
        json!({"focusRequested":true})
    );
    assert!(
        invoke(directory.path(), "shell.exec", json!({"program":"/bin/sh"}))
            .await
            .is_err()
    );
    assert!(
        invoke(
            directory.path(),
            "workspace.focus",
            json!({"path":"/bin/sh"})
        )
        .await
        .is_err()
    );
    assert_eq!(handler.calls.load(Ordering::SeqCst), 1);
    server.shutdown().await?;
    assert!(!directory.path().join("control.sock").exists());
    assert!(!directory.path().join("control.token").exists());
    Ok(())
}
#[tokio::test]
async fn preauth_bad_secret_duplicates_and_oversized_frames_never_dispatch() -> TestResult {
    let directory = directory()?;
    let handler = handler(false);
    let server = Server::start(directory.path(), &Handle::current(), handler.clone())?;
    for wire in [
        cli::encode(&json!({"jsonrpc":"2.0","id":"call","method":"workspace.focus","params":{}}))?,
        cli::encode(
            &json!({"jsonrpc":"2.0","id":"hello","method":"hello","params":{"protocolVersion":1,"token":"0".repeat(64)}}),
        )?,
        {
            let body=br#"{"jsonrpc":"2.0","id":"call","method":"workspace.focus","params":{"x":1,"x":2}}"#;
            let mut value = Zeroizing::new((body.len() as u32).to_le_bytes().to_vec());
            value.extend_from_slice(body);
            value
        },
        Zeroizing::new(((cli::MAX_BODY + 1) as u32).to_le_bytes().to_vec()),
    ] {
        let stream = raw(directory.path()).await?;
        let mut writer = &stream;
        writer.write_all(&wire).await?;
        assert!(
            tokio::time::timeout(Duration::from_secs(1), frame(&stream, None))
                .await?
                .is_err()
        );
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 0);
    server.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn occupied_endpoint_never_rewrites_token() -> TestResult {
    let directory = directory()?;
    let handler = handler(false);
    let server = Server::start(directory.path(), &Handle::current(), handler.clone())?;
    let before = fs::read(directory.path().join("control.token"))?;
    assert!(Server::start(directory.path(), &Handle::current(), handler).is_err());
    assert_eq!(fs::read(directory.path().join("control.token"))?, before);
    server.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn eof_cancels_but_native_credit_stays_until_actual_exit() -> TestResult {
    let directory = directory()?;
    let handler = handler(true);
    let server = Server::start(directory.path(), &Handle::current(), handler.clone())?;
    let stream = raw(directory.path()).await?;
    hello(&stream, directory.path()).await?;
    let mut writer = &stream;
    writer
        .write_all(&cli::encode(
            &json!({"jsonrpc":"2.0","id":"action","method":"workspace.focus","params":{}}),
        )?)
        .await?;
    handler.entered.notified().await;
    drop(stream);
    handler.cancelled.notified().await;
    let shutdown = server.shutdown();
    tokio::pin!(shutdown);
    assert!(
        tokio::time::timeout(Duration::from_millis(20), &mut shutdown)
            .await
            .is_err()
    );
    assert!(directory.path().join("control.sock").exists());
    handler.released.notify_one();
    shutdown.await?;
    assert!(!directory.path().join("control.sock").exists());
    Ok(())
}
#[tokio::test]
async fn unsafe_directory_and_symlink_token_are_not_repaired() -> TestResult {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let directory = directory()?;
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o755))?;
    assert!(Server::start(directory.path(), &Handle::current(), handler(false)).is_err());
    assert_eq!(fs::metadata(directory.path())?.mode() & 0o777, 0o755);
    fs::set_permissions(directory.path(), fs::Permissions::from_mode(0o700))?;
    symlink("absent", directory.path().join("control.token"))?;
    assert!(Server::start(directory.path(), &Handle::current(), handler(false)).is_err());
    assert!(
        fs::symlink_metadata(directory.path().join("control.token"))?
            .file_type()
            .is_symlink()
    );
    Ok(())
}

#[tokio::test]
async fn cancelled_shutdown_observer_does_not_release_native_operation_or_socket() -> TestResult {
    let directory = directory()?;
    let handler = handler(true);
    let server = Arc::new(Server::start(
        directory.path(),
        &Handle::current(),
        handler.clone(),
    )?);
    let stream = raw(directory.path()).await?;
    hello(&stream, directory.path()).await?;
    let mut writer = &stream;
    writer
        .write_all(&cli::encode(
            &json!({"jsonrpc":"2.0","id":"action","method":"workspace.focus","params":{}}),
        )?)
        .await?;
    handler.entered.notified().await;
    let observer = {
        let server = server.clone();
        tokio::spawn(async move { server.shutdown().await })
    };
    handler.cancelled.notified().await;
    observer.abort();
    let _ = observer.await;
    assert!(directory.path().join("control.sock").exists());
    assert!(directory.path().join("control.token").exists());
    handler.released.notify_one();
    server.shutdown().await?;
    assert!(!directory.path().join("control.sock").exists());
    assert!(!directory.path().join("control.token").exists());
    Ok(())
}

#[test]
fn gui_response_budget_does_not_extend_other_commands_or_wire_deadline() {
    assert_eq!(client_budget("workspace.new-tab"), Duration::from_secs(45));
    assert_eq!(client_budget("workspace.split"), Duration::from_secs(45));
    for method in [
        "workspace.focus",
        "workspace.profiles",
        "hello",
        "workspace.unknown",
    ] {
        assert_eq!(client_budget(method), DEADLINE);
    }
    assert_eq!(DEADLINE, Duration::from_secs(5));
}

struct DelayedConsent;
impl Handler for DelayedConsent {
    fn operation_budget(&self, _: &Request) -> Duration {
        Duration::from_secs(45)
    }
    fn call(&self, request: Request, cancelled: CancellationToken) -> Operation {
        Box::pin(async move {
            if !matches!(
                request.method(),
                "workspace.new-tab" | "workspace.split" | "workspace.bootstrap"
            ) {
                return Err(Error);
            }
            tokio::select! { biased;
                _ = cancelled.cancelled() => Err(Error),
                _ = tokio::time::sleep(Duration::from_secs(6)) => Ok(json!({"session":"1"})),
            }
        })
    }
}
#[tokio::test]
async fn actual_socket_gui_commands_wait_past_wire_deadline() -> TestResult {
    let directory = directory()?;
    let server = Server::start(
        directory.path(),
        &Handle::current(),
        Arc::new(DelayedConsent),
    )?;
    for method in ["workspace.new-tab", "workspace.split"] {
        assert_eq!(
            invoke(directory.path(), method, json!({})).await?,
            json!({"session":"1"})
        );
    }
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn cold_bootstrap_uses_bounded_confirmation_response_not_handshake_budget() -> TestResult {
    assert_eq!(
        client_budget("workspace.bootstrap"),
        Duration::from_secs(90)
    );
    assert_eq!(client_budget("workspace.focus"), DEADLINE);
    let directory = directory()?;
    let server = Server::start(
        directory.path(),
        &Handle::current(),
        Arc::new(DelayedConsent),
    )?;
    let reply = invoke(
        directory.path(),
        "workspace.bootstrap",
        json!({"startup":"pending"}),
    )
    .await?;
    assert_eq!(reply, json!({"session":"1"}));
    server.shutdown().await?;
    Ok(())
}

#[tokio::test]
async fn continuous_client_calls_are_independent_and_denial_closes_transport() -> TestResult {
    let directory = directory()?;
    let handler = handler(false);
    let server = Server::start(directory.path(), &Handle::current(), handler.clone())?;
    let mut client = Client::connect(directory.path()).await?;
    for _ in 0..2 {
        assert_eq!(
            client.invoke("workspace.focus", json!({})).await?,
            json!({"focusRequested":true})
        );
    }
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
    assert!(
        client
            .invoke("shell.exec", json!({"program":"/bin/sh"}))
            .await
            .is_err()
    );
    assert!(client.invoke("workspace.focus", json!({})).await.is_err());
    assert_eq!(handler.calls.load(Ordering::SeqCst), 2);
    drop(client);
    server.shutdown().await?;
    Ok(())
}
#[tokio::test]
async fn continuous_client_cannot_follow_replaced_server_instance() -> TestResult {
    let directory = directory()?;
    let first = handler(false);
    let server = Server::start(directory.path(), &Handle::current(), first.clone())?;
    let mut client = Client::connect(directory.path()).await?;
    client.invoke("workspace.focus", json!({})).await?;
    server.shutdown().await?;
    let second = handler(false);
    let replacement = Server::start(directory.path(), &Handle::current(), second.clone())?;
    assert!(client.invoke("workspace.focus", json!({})).await.is_err());
    assert!(client.invoke("workspace.focus", json!({})).await.is_err());
    assert_eq!(first.calls.load(Ordering::SeqCst), 1);
    assert_eq!(second.calls.load(Ordering::SeqCst), 0);
    drop(client);
    replacement.shutdown().await?;
    Ok(())
}
