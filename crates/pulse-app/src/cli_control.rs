//! Opt-in private local control transport; hello never grants GUI or execution authority.
//! Platform adapters admit endpoints and peers; one shared bounded protocol supervises all actions.
use interprocess::local_socket::tokio::{Listener, Stream as IpcStream, prelude::*};
use pulse_ipc::cli::{self, Admission, Request, SessionAuth, WireBuffer};
use serde_json::{Value, json};
use std::{
    future::Future,
    io,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt, ReadBuf},
    runtime::Handle,
    sync::watch,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};
#[cfg(target_os = "linux")]
#[path = "cli_control_unix.rs"]
mod platform;
#[cfg(target_os = "linux")]
use std::path::Path;
/// Static errors contain no token, parameters or user path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("cli.control_failed")]
pub struct Error;
/// Transport result with a static, content-free error.
pub type Result<T> = std::result::Result<T, Error>;
/// Handler futures must retain native work until actual completion, including after cancellation.
pub type Operation = Pin<Box<dyn Future<Output = Result<Value>> + Send>>;
/// Trusted backend only. A successful hello is not authorization to perform an effect.
pub trait Handler: Send + Sync + 'static {
    /// Authorize a typed action independently of the hello gate and retain native work on cancellation.
    fn call(&self, request: Request, cancelled: CancellationToken) -> Operation;
    /// Trusted adapter may allow a separate GUI decision phase. Wire/auth/write stay at 5s.
    /// The transport caps the complete action at 45s; this never grants execution authority.
    fn operation_budget(&self, _request: &Request) -> Duration {
        DEADLINE
    }
}
const DEADLINE: Duration = Duration::from_secs(5);
const MAX_CONNECTIONS: usize = 8;
// Own every accepted/connected stream through cancellation as well as normal close.
// Windows prevents the dependency from retaining a pipe in a detached flush thread.
struct Stream(IpcStream);
impl Drop for Stream {
    fn drop(&mut self) {
        #[cfg(windows)]
        {
            let IpcStream::NamedPipe(stream) = &self.0;
            let pipe: &interprocess::os::windows::named_pipe::tokio::DuplexPipeStream<
                interprocess::os::windows::named_pipe::pipe_mode::Bytes,
            > = stream.as_ref();
            // Protocol receipts govern success. Cancellation closes immediately, without detached flush.
            pipe.assume_flushed();
        }
    }
}
impl AsyncRead for &Stream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut &self.get_mut().0).poll_read(cx, buffer)
    }
}
impl AsyncWrite for &Stream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut &self.get_mut().0).poll_write(cx, bytes)
    }
    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut &self.get_mut().0).poll_flush(cx)
    }
    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut &self.get_mut().0).poll_shutdown(cx)
    }
}
/// Trusted backend endpoint lifecycle; never implement from page-supplied paths or credentials.
/// The desktop adapter retains native directory/file leases until every connection exits.
pub trait NativeEndpoint: Send + 'static {
    /// Admit actual kernel peer metadata before any hello or action. Hello is not Capability admission.
    fn admit_peer(&self, stream: &IpcStream) -> Result<()>;
    /// Release only this instance's endpoints after listener and all native handlers have exited.
    /// Unknown/replaced endpoints must fail closed instead of deleting a later instance's files.
    fn cleanup(&mut self) -> Result<()>;
}
/// Native-only per-instance hello secret. No Debug/serde; it never grants GUI or execution authority.
pub struct TransportToken(Zeroizing<[u8; 64]>);
impl TransportToken {
    /// Generate the same bounded random hello token on every supported platform.
    pub fn generate() -> Self {
        let mut secret = Zeroizing::new([0u8; 64]);
        for index in 0..2 {
            let mut bytes = *uuid::Uuid::new_v4().as_bytes();
            for (offset, byte) in bytes.iter().enumerate() {
                secret[index * 32 + offset * 2] = b"0123456789abcdef"[(byte >> 4) as usize];
                secret[index * 32 + offset * 2 + 1] = b"0123456789abcdef"[(byte & 15) as usize];
            }
            bytes.zeroize();
        }
        Self(secret)
    }
    /// Borrow only for protected native endpoint publication; never log or project these bytes.
    pub fn bytes(&self) -> &[u8; 64] {
        &self.0
    }
}

/// Revocable service handle. Dropping/closing signals cancellation, never reports native completion.
/// Join observers may cancel without canceling the independently owned supervisor.
pub struct Server {
    cancelled: CancellationToken,
    finished: watch::Receiver<Option<Result<()>>>,
}
impl Server {
    /// Backend-supplied OS application runtime directory only; no client-selected path.
    /// Occupied/unknown endpoints fail closed. Cold startup/election and crash recovery are separate.
    #[cfg(target_os = "linux")]
    pub fn start(directory: &Path, runtime: &Handle, handler: Arc<dyn Handler>) -> Result<Self> {
        let secret = TransportToken::generate();
        let (listener, owned) = platform::listen(directory, runtime, secret.bytes())?;
        Ok(Self::start_admitted(
            listener,
            secret,
            Box::new(owned),
            runtime,
            handler,
        ))
    }
    /// Mount an already admitted native listener and its original endpoint leases.
    /// Only trusted backend adapters supply these objects; each action still needs GUI/Capability checks.
    /// Shutdown retains the endpoint until all supervised operations physically finish.
    pub fn start_admitted(
        listener: Listener,
        secret: TransportToken,
        owned: Box<dyn NativeEndpoint>,
        runtime: &Handle,
        handler: Arc<dyn Handler>,
    ) -> Self {
        let cancelled = CancellationToken::new();
        let stopping = cancelled.clone();
        let (done, finished) = watch::channel(None);
        runtime.spawn(async move {
            let result = serve(listener, Arc::new(secret.0), handler, stopping, owned).await;
            done.send_replace(Some(result));
        });
        Self {
            cancelled,
            finished,
        }
    }
    /// Revoke the listener and in-flight requests without claiming native completion.
    pub fn close(&self) {
        self.cancelled.cancel();
    }
    /// Wait for all connection handlers and their supervised native operations to exit.
    pub async fn shutdown(&self) -> Result<()> {
        self.close();
        let mut finished = self.finished.clone();
        loop {
            if let Some(result) = *finished.borrow() {
                return result;
            }
            finished.changed().await.map_err(|_| Error)?;
        }
    }
}
impl Drop for Server {
    fn drop(&mut self) {
        self.close();
    }
}
async fn serve(
    listener: Listener,
    secret: Arc<Zeroizing<[u8; 64]>>,
    handler: Arc<dyn Handler>,
    cancelled: CancellationToken,
    mut endpoints: Box<dyn NativeEndpoint>,
) -> Result<()> {
    let mut tasks = JoinSet::new();
    let mut result = Ok(());
    loop {
        tokio::select! {biased;
            _=cancelled.cancelled()=>break,
            completed=tasks.join_next(),if !tasks.is_empty()=>{if completed.is_some_and(|outcome|outcome.is_err()){result=Err(Error);cancelled.cancel();break;}},
            stream=listener.accept()=>{
                let Ok(stream)=stream.map(Stream) else {result=Err(Error);cancelled.cancel();break;};
                if tasks.len()>=MAX_CONNECTIONS {drop(stream);continue;}
                if endpoints.admit_peer(&stream.0).is_err() {drop(stream);continue;}
                let handler=handler.clone();let secret=secret.clone();let cancelled=cancelled.child_token();
                tasks.spawn(async move {let _=connection(stream,secret,handler,cancelled).await;});
            }
        }
    }
    cancelled.cancel();
    while let Some(completed) = tasks.join_next().await {
        if completed.is_err() {
            result = Err(Error);
        }
    }
    drop(listener);
    endpoints.cleanup()?;
    result
}
async fn frame(stream: &Stream, first: Option<u8>) -> Result<WireBuffer> {
    let mut stream = stream;
    let mut header = [0u8; 4];
    let offset = if let Some(byte) = first {
        header[0] = byte;
        1
    } else {
        0
    };
    stream
        .read_exact(&mut header[offset..])
        .await
        .map_err(|_| Error)?;
    let length = u32::from_le_bytes(header) as usize;
    if length == 0 || length > cli::MAX_BODY {
        return Err(Error);
    }
    let mut body = Zeroizing::new(vec![0; length]);
    stream.read_exact(&mut body).await.map_err(|_| Error)?;
    Ok(body)
}
async fn connection(
    stream: Stream,
    secret: Arc<Zeroizing<[u8; 64]>>,
    handler: Arc<dyn Handler>,
    cancelled: CancellationToken,
) -> Result<()> {
    let mut auth = SessionAuth::new(Zeroizing::new(**secret));
    let mut first = None;
    loop {
        let body = tokio::select! {biased;_=cancelled.cancelled()=>return Ok(()),value=tokio::time::timeout(DEADLINE,frame(&stream,first.take()))=>value.map_err(|_|Error)??};
        let request = Request::parse(&body).map_err(|_| Error)?;
        drop(body);
        let (id, result) = match auth.admit(request).map_err(|_| Error)? {
            Admission::Hello { id } => (id, Ok(json!({"protocolVersion":1}))),
            Admission::Call(request) => {
                let id = request.id().to_owned();
                let stop = cancelled.child_token();
                let budget = handler
                    .operation_budget(&request)
                    .clamp(DEADLINE, Duration::from_secs(45));
                let operation = handler.call(request, stop.clone());
                tokio::pin!(operation);
                let deadline = tokio::time::sleep(budget);
                tokio::pin!(deadline);
                let mut byte = [0u8; 1];
                let mut reader = &stream;
                let value = tokio::select! {biased;
                    _=cancelled.cancelled()=>{stop.cancel();let _=operation.await;return Ok(());},
                    value=&mut operation=>value,
                    _=&mut deadline=>{stop.cancel();let _=operation.await;return Err(Error);},
                    count=reader.read(&mut byte)=>{
                        if count.ok()!=Some(1){stop.cancel();let _=operation.await;return Err(Error);}
                        first=Some(byte[0]);
                        tokio::select! {biased;_=cancelled.cancelled()=>{stop.cancel();let _=operation.await;return Ok(());},value=&mut operation=>value,_=&mut deadline=>{stop.cancel();let _=operation.await;return Err(Error);}}
                    }
                };
                (id, value)
            }
        };
        let reply = match result {
            Ok(value) => json!({"jsonrpc":"2.0","id":id,"result":value}),
            Err(_) => {
                json!({"jsonrpc":"2.0","id":id,"error":{"code":-32000,"message":"cli.denied"}})
            }
        };
        let wire = cli::encode(&reply).map_err(|_| Error)?;
        let mut writer = &stream;
        tokio::select! {biased;_=cancelled.cancelled()=>return Ok(()),value=tokio::time::timeout(DEADLINE,writer.write_all(&wire))=>value.map_err(|_|Error)?.map_err(|_|Error)?};
    }
}
fn client_budget(method: &str) -> Duration {
    if method == "workspace.bootstrap" {
        return Duration::from_secs(90);
    }
    if matches!(
        method,
        "workspace.new-tab" | "workspace.new-window" | "workspace.split" | "workspace.ssh"
    ) {
        Duration::from_secs(45)
    } else {
        DEADLINE
    }
}
/// One authenticated transport to one server instance. Each call still needs
/// its own Capability and original-page approval. Any failure closes the client.
pub struct Client {
    stream: Option<Stream>,
    next_id: usize,
}
impl Client {
    /// Authenticate once against the private endpoint without creating or repairing it.
    #[cfg(target_os = "linux")]
    pub async fn connect(directory: &Path) -> Result<Self> {
        let stream = tokio::time::timeout(DEADLINE, authenticate(directory))
            .await
            .map_err(|_| Error)??;
        Ok(Self {
            stream: Some(stream),
            next_id: 1,
        })
    }
    /// Authenticate a platform-admitted, overlapped Windows handle with IDENTIFICATION QOS.
    /// The trusted adapter must derive the pipe from its private directory and admit the actual PID.
    /// No page/Capability authority is granted. Cancellation closes the handle without detached flush.
    #[cfg(windows)]
    pub async fn connect_native(
        handle: std::os::windows::io::OwnedHandle,
        secret: Zeroizing<Vec<u8>>,
        admit_peer: impl FnOnce(u32) -> Result<()> + Send,
    ) -> Result<Self> {
        let stream = Stream(IpcStream::NamedPipe(handle.try_into().map_err(|_| Error)?));
        if secret.len() != 64
            || !secret
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            return Err(Error);
        }
        admit_peer(
            stream
                .0
                .peer_creds()
                .map_err(|_| Error)?
                .pid()
                .ok_or(Error)?,
        )?;
        let stream = tokio::time::timeout(DEADLINE, authenticate_stream(stream, secret))
            .await
            .map_err(|_| Error)??;
        Ok(Self {
            stream: Some(stream),
            next_id: 1,
        })
    }

    /// Invoke once with a fresh correlation ID; error or cancellation retires this client.
    pub async fn invoke(&mut self, method: &str, params: Value) -> Result<Value> {
        // Taking ownership also closes the stream when this waiter is dropped.
        let stream = self.stream.take().ok_or(Error)?;
        if self.next_id >= cli::MAX_REQUESTS {
            return Err(Error);
        }
        let id = format!("action-{}", self.next_id);
        self.next_id += 1;
        let result = tokio::time::timeout(
            client_budget(method),
            invoke_stream(&stream, &id, method, params),
        )
        .await
        .map_err(|_| Error)?;
        if result.is_ok() {
            self.stream = Some(stream);
        }
        result
    }
}
/// Warm client. Endpoint validation never creates or repairs a directory/token.
#[cfg(target_os = "linux")]
pub async fn invoke(directory: &Path, method: &str, params: Value) -> Result<Value> {
    let budget = client_budget(method);
    tokio::time::timeout(budget, invoke_inner(directory, method, params))
        .await
        .map_err(|_| Error)?
}
#[cfg(target_os = "linux")]
async fn authenticate(directory: &Path) -> Result<Stream> {
    let (stream, secret) = platform::connect(directory).await?;
    authenticate_stream(stream, secret).await
}
async fn authenticate_stream(stream: Stream, secret: Zeroizing<Vec<u8>>) -> Result<Stream> {
    let mut hello = json!({"jsonrpc":"2.0","id":"hello","method":"hello","params":{"protocolVersion":1,"token":String::from_utf8(secret.to_vec()).map_err(|_|Error)?}});
    let wire = cli::encode(&hello).map_err(|_| Error)?;
    if let Some(Value::String(token)) = hello["params"]
        .as_object_mut()
        .and_then(|params| params.get_mut("token"))
    {
        token.zeroize();
    }
    drop(hello);
    drop(secret);
    let mut writer = &stream;
    writer.write_all(&wire).await.map_err(|_| Error)?;
    drop(wire);
    let body = frame(&stream, None).await?;
    let response: Value = serde_json::from_slice(&body).map_err(|_| Error)?;
    if response["jsonrpc"] != "2.0"
        || response["id"] != "hello"
        || response.get("error").is_some()
        || response["result"]["protocolVersion"] != 1
    {
        return Err(Error);
    }
    Ok(stream)
}
#[cfg(target_os = "linux")]
async fn invoke_inner(directory: &Path, method: &str, params: Value) -> Result<Value> {
    let stream = tokio::time::timeout(DEADLINE, authenticate(directory))
        .await
        .map_err(|_| Error)??;
    invoke_stream(&stream, "action", method, params).await
}
async fn invoke_stream(stream: &Stream, id: &str, method: &str, params: Value) -> Result<Value> {
    let mut writer = stream;
    let wire = cli::encode(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
        .map_err(|_| Error)?;
    tokio::time::timeout(DEADLINE, writer.write_all(&wire))
        .await
        .map_err(|_| Error)?
        .map_err(|_| Error)?;
    drop(wire);
    let body = frame(stream, None).await?;
    let response: Value = serde_json::from_slice(&body).map_err(|_| Error)?;
    if response["jsonrpc"] != "2.0" || response["id"] != id || response.get("error").is_some() {
        return Err(Error);
    }
    response.get("result").cloned().ok_or(Error)
}
#[cfg(all(test, unix))]
mod tests;
