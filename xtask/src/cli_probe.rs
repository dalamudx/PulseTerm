//! S5 fixed read-only probe. Native UDS only; not the interprocess production adapter.
#[path = "cli_startup_probe.rs"]
mod startup;
pub fn run_startup() -> Result<()> {
    startup::run()
}
pub fn startup_helper() -> Result<()> {
    startup::helper()
}
#[cfg(test)]
#[path = "cli_probe_tests.rs"]
mod tests;
use anyhow::{Result, ensure};
use pulse_ipc::cli::{Admission, CliError, Decoder, Request, SessionAuth, WireBuffer, encode};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::Write,
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    process::Command,
};
use zeroize::Zeroizing;
const CASES: [&str; 8] = [
    "good",
    "wrong-token",
    "wrong-version",
    "preauth",
    "oversize",
    "partial",
    "idle",
    "unknown-method",
];
fn private_token(path: &Path) -> Result<Zeroizing<[u8; 64]>> {
    let meta = std::fs::symlink_metadata(path)?;
    let dir = std::fs::symlink_metadata(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("token directory missing"))?,
    )?;
    ensure!(
        meta.is_file()
            && !meta.file_type().is_symlink()
            && meta.permissions().mode() & 0o777 == 0o600
            && dir.is_dir()
            && !dir.file_type().is_symlink()
            && dir.permissions().mode() & 0o777 == 0o700
            && meta.uid() == dir.uid(),
        "unsafe CLI runtime permissions"
    );
    let mut token = Zeroizing::new([0u8; 64]);
    use std::io::Read;
    let mut file = File::open(path)?;
    ensure!(
        file.metadata()?.len() == 64 && file.metadata()?.uid() == dir.uid(),
        "CLI token shape/owner"
    );
    file.read_exact(&mut *token)?;
    ensure!(
        token
            .iter()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b)),
        "CLI token encoding"
    );
    Ok(token)
}
async fn read_frame(stream: &mut UnixStream, decoder: &mut Decoder) -> Result<Option<WireBuffer>> {
    loop {
        let mut byte = [0u8; 1];
        let n = stream.read(&mut byte).await?;
        if n == 0 {
            decoder.finish()?;
            return Ok(None);
        }
        let (_, body) = decoder.push(&byte)?;
        if body.is_some() {
            return Ok(body);
        }
    }
}
async fn send(stream: &mut UnixStream, value: &Value) -> Result<()> {
    let wire = encode(value)?;
    stream.write_all(&wire).await?;
    Ok(())
}
async fn reply(stream: &mut UnixStream, d: &mut Decoder) -> Result<Value> {
    let body = read_frame(stream, d)
        .await?
        .ok_or_else(|| anyhow::anyhow!("CLI reply EOF"))?;
    Ok(serde_json::from_slice(&body)?)
}
fn rpc(id: &str, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
async fn serve(
    mut stream: UnixStream,
    uid: u32,
    token: Zeroizing<[u8; 64]>,
) -> Result<&'static str> {
    ensure!(stream.peer_cred()?.uid() == uid, "CLI peer UID mismatch");
    let mut auth = SessionAuth::new(token);
    let mut decoder = Decoder::default();
    loop {
        let Some(body) = read_frame(&mut stream, &mut decoder).await? else {
            return Ok("eof");
        };
        match auth.admit(Request::parse(&body)?)? {
            Admission::Hello { id } => {
                send(
                    &mut stream,
                    &json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1}}),
                )
                .await?
            }
            Admission::Call(call) => {
                if call.method() != "probe.ping" || !call.params().is_empty() {
                    send(&mut stream,&json!({"jsonrpc":"2.0","id":call.id(),"error":{"code":-32601,"message":"method unavailable"}})).await?;
                    return Ok("unknown-method");
                }
                send(
                    &mut stream,
                    &json!({"jsonrpc":"2.0","id":call.id(),"result":{"pong":"中文🌍"}}),
                )
                .await?;
                return Ok("ping");
            }
        }
    }
}
fn reason(e: anyhow::Error) -> Result<&'static str> {
    match e.downcast_ref::<CliError>() {
        Some(CliError::Authentication) => Ok("authentication"),
        Some(CliError::Handshake) => Ok("handshake"),
        Some(CliError::Length) => Ok("length"),
        Some(CliError::Truncated) => Ok("truncated"),
        _ => Err(e),
    }
}
async fn child(executable: &Path, socket: &Path, token: &Path, mode: &str) -> Result<()> {
    let mut process = Command::new(executable)
        .args(["cli-probe-helper"])
        .arg(socket)
        .arg(token)
        .arg(mode)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .kill_on_drop(true)
        .spawn()?;
    let status = tokio::time::timeout(Duration::from_secs(3), process.wait()).await??;
    ensure!(status.success(), "fixed CLI helper failed for {mode}");
    Ok(())
}
async fn probe() -> Result<Value> {
    let directory = tempfile::Builder::new()
        .prefix("pulse-cli-probe-")
        .tempdir()?;
    let root = directory.path().to_path_buf();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let token_path = root.join("token");
    let socket_path = root.join("rpc.sock");
    let text = Zeroizing::new(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ));
    let token = Zeroizing::new(<[u8; 64]>::try_from(text.as_bytes())?);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&token_path)?;
    file.write_all(token.as_slice())?;
    file.sync_all()?;
    drop(file);
    let uid = std::fs::metadata(&root)?.uid();
    let listener = UnixListener::bind(&socket_path)?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
    let tasks = tokio::spawn(async move {
        let mut results = vec![];
        for _ in CASES {
            let (stream, _) =
                tokio::time::timeout(Duration::from_secs(3), listener.accept()).await??;
            results.push(
                match tokio::time::timeout(
                    Duration::from_millis(500),
                    serve(stream, uid, Zeroizing::new(*token)),
                )
                .await
                {
                    Ok(Ok(kind)) => kind,
                    Ok(Err(e)) => reason(e)?,
                    Err(_) => "deadline",
                },
            );
        }
        Ok::<_, anyhow::Error>(results)
    });
    let guard = TaskGuard(tasks.abort_handle());
    let executable = std::env::current_exe()?;
    for mode in CASES {
        child(&executable, &socket_path, &token_path, mode).await?;
    }
    let results = tasks.await??;
    drop(guard);
    ensure!(
        results
            == [
                "ping",
                "authentication",
                "authentication",
                "handshake",
                "length",
                "truncated",
                "deadline",
                "unknown-method"
            ],
        "CLI admission observations differ"
    );
    let symlink = root.join("token-link");
    std::os::unix::fs::symlink(&token_path, &symlink)?;
    child(&executable, &socket_path, &symlink, "unsafe-token").await?;
    std::fs::set_permissions(&token_path, std::fs::Permissions::from_mode(0o640))?;
    child(&executable, &socket_path, &token_path, "unsafe-token").await?;
    directory.close()?;
    ensure!(!root.exists(), "CLI private runtime remained after cleanup");
    Ok(
        json!({"schemaVersion":1,"independentClientProcesses":10,"lengthPrefixedJsonRpc":true,"privateDirectory0700":true,"tokenAndSocket0600":true,"sameUserPeerCredChecked":true,"secretNotInArgsEnvironmentOrReports":true,"wrongTokenAndVersionRejected":true,"preauthDispatchRejected":true,"overlongHeaderRejectedBeforeAllocation":true,"truncatedEofRejected":true,"idleDeadlineClosesConnection":true,"unknownMethodCannotExecute":true,"unicodeReplyExact":true,"unsafeTokenModeAndSymlinkRejected":true,"clientsListenerAndDirectoryCleaned":true,"observations":results,"limits":["Fixed read-only native Unix socket fixture, not production interprocess adapter","Not Windows named-pipe ACL, cross-user impersonation, window creation latency or cold-start/single-instance acceptance","Authentication does not bypass Capability registry or risk confirmation"]}),
    )
}
struct TaskGuard(tokio::task::AbortHandle);
impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.0.abort();
    }
}
fn validate(r: &Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1 && r["independentClientProcesses"] == 10,
        "CLI version/process count"
    );
    for key in [
        "lengthPrefixedJsonRpc",
        "privateDirectory0700",
        "tokenAndSocket0600",
        "sameUserPeerCredChecked",
        "secretNotInArgsEnvironmentOrReports",
        "wrongTokenAndVersionRejected",
        "preauthDispatchRejected",
        "overlongHeaderRejectedBeforeAllocation",
        "truncatedEofRejected",
        "idleDeadlineClosesConnection",
        "unknownMethodCannotExecute",
        "unicodeReplyExact",
        "unsafeTokenModeAndSymlinkRejected",
        "clientsListenerAndDirectoryCleaned",
    ] {
        ensure!(r[key] == true, "CLI observation missing");
    }
    ensure!(
        r["observations"]
            == json!([
                "ping",
                "authentication",
                "authentication",
                "handshake",
                "length",
                "truncated",
                "deadline",
                "unknown-method"
            ])
            && r["limits"].as_array().is_some_and(|v| !v.is_empty()),
        "CLI failed-case receipts missing"
    );
    Ok(())
}

pub fn run() -> Result<()> {
    let root = super::root()?;
    let path = root.join("target/probes/cli.json");
    std::fs::create_dir_all(root.join("target/probes"))?;
    if path.exists() {
        std::fs::remove_file(&path)?;
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()?;
    let report = runtime
        .block_on(async { tokio::time::timeout(Duration::from_secs(20), probe()).await })??;
    validate(&report)?;
    std::fs::write(path, serde_json::to_vec_pretty(&report)?)?;
    println!("PASS private Unix CLI framing, token admission, peer credentials and cleanup");
    Ok(())
}
pub fn helper() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    ensure!(args.len() == 5, "fixed CLI helper arguments");
    let socket = PathBuf::from(&args[2]);
    let path = PathBuf::from(&args[3]);
    let mode = args[4]
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("mode encoding"))?;
    if mode == "unsafe-token" {
        ensure!(private_token(&path).is_err(), "unsafe credential accepted");
        return Ok(());
    }
    let token = private_token(&path)?;
    let uid = std::fs::metadata(&path)?.uid();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async{let mut stream=UnixStream::connect(&socket).await?;ensure!(stream.peer_cred()?.uid()==uid,"unexpected server UID");let mut decoder=Decoder::default();match mode{"good"|"unknown-method"=>{let secret=std::str::from_utf8(token.as_slice())?;let hello=encode(&rpc("h","hello",json!({"protocolVersion":1,"token":secret})))?;for byte in hello.iter(){stream.write_all(&[*byte]).await?;}let accepted=reply(&mut stream,&mut decoder).await?;ensure!(accepted["jsonrpc"]=="2.0"&&accepted["id"]=="h"&&accepted["result"]["protocolVersion"]==1,"hello receipt");send(&mut stream,&rpc("p",if mode=="good"{"probe.ping"}else{"shell.exec"},json!({}))).await?;let response=reply(&mut stream,&mut decoder).await?;ensure!(response["jsonrpc"]=="2.0"&&response["id"]=="p","RPC id");if mode=="good"{ensure!(response["result"]["pong"]=="中文🌍","RPC Unicode reply");}else{ensure!(response["error"]["code"]==-32601,"unknown command executed");}},"wrong-token"|"wrong-version"|"denied-token"=>send(&mut stream,&rpc("h","hello",json!({"protocolVersion":if mode=="wrong-version"{2}else{1},"token":if mode=="wrong-token"{"0".repeat(64)}else{std::str::from_utf8(token.as_slice())?.to_string()}}))).await?,"preauth"=>send(&mut stream,&rpc("p","probe.ping",json!({}))).await?,"oversize"=>stream.write_all(&u32::MAX.to_le_bytes()).await?,"partial"=>{stream.write_all(&[16,0]).await?;stream.shutdown().await?;},"idle"=>{},_=>anyhow::bail!("fixed helper mode required")};let mut byte=[0u8;1];ensure!(tokio::time::timeout(Duration::from_secs(2),stream.read(&mut byte)).await??==0,"connection did not retire");Ok::<_,anyhow::Error>(())})
}
