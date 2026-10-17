//! Opt-in warm Linux CLI transport. No bootstrap, stale-endpoint deletion or arbitrary execution.
//! Backend handlers must separately authorize the exact original target and native commit.
use interprocess::{
    local_socket::{
        GenericFilePath, ListenerOptions,
        tokio::{Listener, Stream, prelude::*},
    },
    os::unix::local_socket::ListenerOptionsExt,
};
use pulse_ipc::cli::{self, Admission, Request, SessionAuth, WireBuffer};
use serde_json::{Value, json};
use std::{
    fs::{self, Metadata, OpenOptions},
    future::Future,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    pin::Pin,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    runtime::Handle,
    sync::watch,
    task::JoinSet,
};
use tokio_util::sync::CancellationToken;
use zeroize::{Zeroize, Zeroizing};
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
struct Identity {
    path: PathBuf,
    device: u64,
    inode: u64,
}
impl Identity {
    fn new(path: PathBuf, metadata: &Metadata) -> Self {
        Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        }
    }
    fn remove(&self) -> Result<()> {
        match fs::symlink_metadata(&self.path) {
            Ok(meta) if meta.dev() == self.device && meta.ino() == self.inode => {
                fs::remove_file(&self.path).map_err(|_| Error)
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            _ => Err(Error),
        }
    }
}
struct Endpoints {
    socket: Option<Identity>,
    token: Option<Identity>,
}
impl Endpoints {
    fn cleanup(&mut self) -> Result<()> {
        let socket = self
            .socket
            .take()
            .map_or(Ok(()), |identity| identity.remove());
        let token = self
            .token
            .take()
            .map_or(Ok(()), |identity| identity.remove());
        socket.and(token)
    }
}
impl Drop for Endpoints {
    fn drop(&mut self) {
        let _ = self.cleanup();
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
    pub fn start(directory: &Path, runtime: &Handle, handler: Arc<dyn Handler>) -> Result<Self> {
        let directory = pulse_store::state_directory::prepare(directory).map_err(|_| Error)?;
        let socket_path = directory.join("control.sock");
        let token_path = directory.join("control.token");
        for path in [&socket_path, &token_path] {
            match fs::symlink_metadata(path) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                _ => return Err(Error),
            }
        }
        let mut owned = Endpoints {
            socket: None,
            token: None,
        };
        let name = socket_path
            .as_path()
            .to_fs_name::<GenericFilePath>()
            .map_err(|_| Error)?;
        let listener = {
            let _entered = runtime.enter();
            ListenerOptions::new()
                .name(name)
                .mode(0o600)
                .reclaim_name(false)
                .try_overwrite(false)
                .create_tokio()
                .map_err(|_| Error)?
        };
        owned.socket = Some(Identity::new(
            socket_path.clone(),
            &fs::symlink_metadata(&socket_path).map_err(|_| Error)?,
        ));
        let mut secret = Zeroizing::new([0u8; 64]);
        for index in 0..2 {
            let mut bytes = *uuid::Uuid::new_v4().as_bytes();
            for (offset, byte) in bytes.iter().enumerate() {
                secret[index * 32 + offset * 2] = b"0123456789abcdef"[(byte >> 4) as usize];
                secret[index * 32 + offset * 2 + 1] = b"0123456789abcdef"[(byte & 15) as usize];
            }
            bytes.zeroize();
        }
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&token_path)
            .map_err(|_| Error)?;
        owned.token = Some(Identity::new(
            token_path,
            &file.metadata().map_err(|_| Error)?,
        ));
        file.write_all(secret.as_ref()).map_err(|_| Error)?;
        file.sync_all().map_err(|_| Error)?;
        drop(file);
        let cancelled = CancellationToken::new();
        let stopping = cancelled.clone();
        let (done, finished) = watch::channel(None);
        runtime.spawn(async move {
            let result = serve(listener, Arc::new(secret), handler, stopping, owned).await;
            done.send_replace(Some(result));
        });
        Ok(Self {
            cancelled,
            finished,
        })
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
    mut endpoints: Endpoints,
) -> Result<()> {
    let mut tasks = JoinSet::new();
    let mut result = Ok(());
    loop {
        tokio::select! {biased;
            _=cancelled.cancelled()=>break,
            completed=tasks.join_next(),if !tasks.is_empty()=>{if completed.is_some_and(|outcome|outcome.is_err()){result=Err(Error);cancelled.cancel();break;}},
            stream=listener.accept()=>{
                let Ok(stream)=stream else {result=Err(Error);cancelled.cancel();break;};
                if tasks.len()>=MAX_CONNECTIONS {drop(stream);continue;}
                if stream.peer_creds().ok().and_then(|peer|peer.euid())!=Some(rustix::process::geteuid().as_raw()) {drop(stream);continue;}
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
    pub async fn connect(directory: &Path) -> Result<Self> {
        let stream = tokio::time::timeout(DEADLINE, authenticate(directory))
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
pub async fn invoke(directory: &Path, method: &str, params: Value) -> Result<Value> {
    let budget = client_budget(method);
    tokio::time::timeout(budget, invoke_inner(directory, method, params))
        .await
        .map_err(|_| Error)?
}
async fn authenticate(directory: &Path) -> Result<Stream> {
    let socket = directory.join("control.sock");
    let token = directory.join("control.token");
    // Runtime is a backend-created private directory; reject links through every ancestor.
    if !directory.is_absolute() {
        return Err(Error);
    }
    let mut path = PathBuf::new();
    for component in directory.components() {
        match component {
            std::path::Component::RootDir | std::path::Component::Normal(_) => {
                path.push(component.as_os_str())
            }
            _ => return Err(Error),
        }
        let meta = fs::symlink_metadata(&path).map_err(|_| Error)?;
        if !meta.is_dir() {
            return Err(Error);
        }
        if meta.uid() != 0 && meta.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error);
        }
        if meta.mode() & 0o022 != 0 && !(meta.uid() == 0 && meta.mode() & 0o1777 == 0o1777) {
            return Err(Error);
        }
    }
    let meta = fs::symlink_metadata(directory).map_err(|_| Error)?;
    if meta.uid() != rustix::process::geteuid().as_raw() || meta.mode() & 0o7777 != 0o700 {
        return Err(Error);
    }
    let socket_meta = fs::symlink_metadata(&socket).map_err(|_| Error)?;
    use std::os::unix::fs::FileTypeExt;
    if !socket_meta.file_type().is_socket()
        || socket_meta.uid() != meta.uid()
        || socket_meta.mode() & 0o7777 != 0o600
        || socket_meta.nlink() != 1
    {
        return Err(Error);
    }
    let token_meta = fs::symlink_metadata(&token).map_err(|_| Error)?;
    if !token_meta.is_file()
        || token_meta.uid() != meta.uid()
        || token_meta.mode() & 0o7777 != 0o600
        || token_meta.nlink() != 1
        || token_meta.len() != 64
    {
        return Err(Error);
    }
    // O_NOFOLLOW closes the final-link boundary; the same-UID directory race is not claimed solved.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&token)
        .map_err(|_| Error)?;
    let opened = file.metadata().map_err(|_| Error)?;
    if opened.dev() != token_meta.dev() || opened.ino() != token_meta.ino() {
        return Err(Error);
    }
    let mut secret = Zeroizing::new(Vec::with_capacity(65));
    file.take(65).read_to_end(&mut secret).map_err(|_| Error)?;
    if secret.len() != 64
        || !secret
            .iter()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(byte))
    {
        return Err(Error);
    }
    let stream = Stream::connect(socket.to_fs_name::<GenericFilePath>().map_err(|_| Error)?)
        .await
        .map_err(|_| Error)?;
    if stream.peer_creds().ok().and_then(|peer| peer.euid()) != Some(meta.uid()) {
        return Err(Error);
    }
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
#[cfg(test)]
mod tests;
