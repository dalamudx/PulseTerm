//! Fixed CLI-to-native-window timing probe. No arbitrary window/shell/capability action.
use pulse_ipc::cli::{Admission, Decoder, Request, SessionAuth, WireBuffer, encode};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::PathBuf,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tauri::{Manager, WebviewUrl, WebviewWindow, WebviewWindowBuilder, WindowEvent};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    sync::oneshot,
};
use zeroize::Zeroizing;
type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn read_frame(stream: &mut UnixStream, d: &mut Decoder) -> Result<WireBuffer> {
    loop {
        let mut b = [0u8; 1];
        if stream.read(&mut b).await? == 0 {
            d.finish()?;
            return Err("cliwindow.eof".into());
        }
        let (_, body) = d.push(&b)?;
        if let Some(body) = body {
            return Ok(body);
        }
    }
}
async fn send(stream: &mut UnixStream, value: &Value) -> Result<()> {
    stream.write_all(&encode(value)?).await?;
    Ok(())
}
fn rpc(id: &str, method: &str, params: Value) -> Value {
    json!({"jsonrpc":"2.0","id":id,"method":method,"params":params})
}
fn micros(start: Instant) -> u64 {
    u64::try_from(start.elapsed().as_micros()).unwrap_or(u64::MAX)
}
struct WindowGuard(WebviewWindow);
impl Drop for WindowGuard {
    fn drop(&mut self) {
        let _ = self.0.close();
    }
}
async fn window(
    app: tauri::AppHandle,
    label: String,
    stream: &mut UnixStream,
    id: &str,
) -> Result<Value> {
    let requested = Instant::now();
    let (tx, rx) = oneshot::channel();
    let (page_tx, page_rx) = oneshot::channel();
    let (page_tx, destroyed_tx) = (
        Arc::new(Mutex::new(Some(page_tx))),
        Arc::new(Mutex::new(None)),
    );
    let (destroy_tx, destroy_rx) = oneshot::channel();
    *destroyed_tx.lock().map_err(|_| "cliwindow.lock")? = Some(destroy_tx);
    let gui = app.clone();
    app.run_on_main_thread(move || {
        if tx.is_closed() {
            return;
        }
        let built = (|| -> Result<WebviewWindow> {
            let w = WebviewWindowBuilder::new(
                &gui,
                label,
                WebviewUrl::App("index.html?session-peer=1".into()),
            )
            .title("PulseTerm CLI fixed validation")
            .inner_size(480.0, 320.0)
            .visible(true)
            .on_page_load(move |_, payload| {
                if matches!(payload.event(), tauri::webview::PageLoadEvent::Finished)
                    && let Ok(mut tx) = page_tx.lock()
                    && let Some(tx) = tx.take()
                {
                    let _ = tx.send(micros(requested));
                }
            })
            .build()?;
            let target = destroyed_tx.clone();
            w.on_window_event(move |e| {
                if matches!(e, WindowEvent::Destroyed)
                    && let Ok(mut tx) = target.lock()
                    && let Some(tx) = tx.take()
                {
                    let _ = tx.send(());
                }
            });
            Ok(w)
        })();
        let built_us = micros(requested);
        let cleanup = built.as_ref().ok().cloned();
        if tx.send((built, built_us)).is_err()
            && let Some(window) = cleanup
        {
            let _ = window.close();
        }
    })?;
    let (built, built_us) = tokio::time::timeout(Duration::from_secs(3), rx).await??;
    let w = built?;
    let _guard = WindowGuard(w.clone());
    if !w.is_visible()? || app.get_webview_window(w.label()).is_none() {
        return Err("cliwindow.not_visible_registered".into());
    }
    let native = json!({"nativeVisibleRegistered":true,"serverToNativeUs":built_us});
    // Registration receipt is not page-ready/paint: keep those timestamps separate.
    send(stream, &json!({"jsonrpc":"2.0","id":id,"result":native})).await?;
    let loaded_us = tokio::time::timeout(Duration::from_secs(5), page_rx).await??;
    w.close()?;
    tokio::time::timeout(Duration::from_secs(3), destroy_rx).await??;
    if app.get_webview_window(w.label()).is_some() {
        return Err("cliwindow.window_not_destroyed".into());
    }
    Ok(json!({"native":native,"pageLoadedUs":loaded_us,"nativeDestroyed":true}))
}
async fn serve(
    mut stream: UnixStream,
    uid: u32,
    token: Zeroizing<[u8; 64]>,
    app: tauri::AppHandle,
    label: String,
) -> Result<Value> {
    if stream.peer_cred()?.uid() != uid {
        return Err("cliwindow.peer_uid".into());
    }
    let mut d = Decoder::default();
    let mut auth = SessionAuth::new(token);
    let hello = Request::parse(&read_frame(&mut stream, &mut d).await?)?;
    let id = match auth.admit(hello) {
        Ok(Admission::Hello { id }) => id,
        Err(_) => return Ok(json!({"rejected":true})),
        _ => return Err("cliwindow.hello".into()),
    };
    send(
        &mut stream,
        &json!({"jsonrpc":"2.0","id":id,"result":{"protocolVersion":1}}),
    )
    .await?;
    let request = Request::parse(&read_frame(&mut stream, &mut d).await?)?;
    let call = match auth.admit(request)? {
        Admission::Call(call) => call,
        _ => return Err("cliwindow.call".into()),
    };
    if call.method() != "probe.window" || !call.params().is_empty() {
        return Err("cliwindow.fixed_action_only".into());
    }
    window(app, label, &mut stream, call.id()).await
}
async fn measure(app: tauri::AppHandle) -> Result<Value> {
    let directory = tempfile::Builder::new()
        .prefix("pulse-cli-window-")
        .tempdir()?;
    let root = directory.path().to_path_buf();
    std::fs::set_permissions(&root, std::fs::Permissions::from_mode(0o700))?;
    let token_path = root.join("token");
    let socket_path = root.join("rpc.sock");
    let secret = Zeroizing::new(format!(
        "{}{}",
        uuid::Uuid::new_v4().simple(),
        uuid::Uuid::new_v4().simple()
    ));
    let token = Zeroizing::new(<[u8; 64]>::try_from(secret.as_bytes())?);
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&token_path)?;
    file.write_all(token.as_slice())?;
    drop(file);
    let listener = UnixListener::bind(&socket_path)?;
    std::fs::set_permissions(&socket_path, std::fs::Permissions::from_mode(0o600))?;
    let uid = std::fs::metadata(&root)?.uid();
    let initial = app.webview_windows().len();
    let mut samples = vec![];
    for (index, mode) in ["denied", "good", "good"].iter().enumerate() {
        let child = tokio::process::Command::new(std::env::current_exe()?)
            .arg("--cli-window-helper")
            .arg(&socket_path)
            .arg(&token_path)
            .arg(mode)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let (stream, _) = tokio::time::timeout(Duration::from_secs(3), listener.accept()).await??;
        let native = tokio::time::timeout(
            Duration::from_secs(10),
            serve(
                stream,
                uid,
                Zeroizing::new(*token),
                app.clone(),
                format!("cli-fixed-{index}"),
            ),
        )
        .await??;
        let output =
            tokio::time::timeout(Duration::from_secs(3), child.wait_with_output()).await??;
        if !output.status.success() || output.stdout.len() > 4096 {
            return Err("cliwindow.helper_failed".into());
        }
        let client: Value = serde_json::from_slice(&output.stdout)?;
        if app.webview_windows().len() != initial {
            return Err("cliwindow.window_leak".into());
        }
        if index == 0 {
            if native["rejected"] != true || client["denied"] != true {
                return Err("cliwindow.bad_token_action".into());
            }
        } else {
            samples.push(json!({"native":native,"client":client}));
        }
    }
    drop(listener);
    directory.close()?;
    if root.exists() {
        return Err("cliwindow.runtime_leftover".into());
    }
    Ok(
        json!({"schemaVersion":1,"completed":true,"mainPageReadyBeforeClient":true,"badTokenCreatedNoWindow":true,"samples":samples,"clientsListenerWindowsAndDirectoryCleaned":true,"limits":["Explicit native Unix CLI fixture, not production CLI/interprocess/Windows named pipes","Server native-visible and page-load timestamps are separate; neither proves compositor first-pixel time","Reply confirms native registration/visible state only; page-load and destruction are independently supervised","Not cold-start/single-instance/real-user window latency acceptance"]}),
    )
}
pub(crate) fn launch(app: tauri::AppHandle, path: PathBuf) {
    tauri::async_runtime::spawn(async move {
        let result = tokio::time::timeout(Duration::from_secs(25), measure(app.clone())).await;
        let (report, exit) = match result {
            Ok(Ok(report)) => (report, 0),
            _ => (
                json!({"schemaVersion":1,"completed":false,"error":"cliwindow.probe_failed"}),
                1,
            ),
        };
        if let Ok(bytes) = serde_json::to_vec_pretty(&report) {
            let _ = std::fs::write(path, bytes);
        }
        app.exit(exit);
    });
}
pub(crate) fn helper() -> Result<()> {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() != 5 {
        return Err("cliwindow.helper_args".into());
    }
    let socket = PathBuf::from(&args[2]);
    let token_path = PathBuf::from(&args[3]);
    if args[4] != "denied" && args[4] != "good" {
        return Err("cliwindow.fixed_helper_mode".into());
    }
    let denied = args[4] == "denied";
    let meta = std::fs::symlink_metadata(&token_path)?;
    if !meta.is_file()
        || meta.file_type().is_symlink()
        || meta.permissions().mode() & 0o777 != 0o600
        || meta.len() != 64
    {
        return Err("cliwindow.token_mode".into());
    }
    let mut token = Zeroizing::new([0u8; 64]);
    std::fs::File::open(&token_path)?.read_exact(token.as_mut_slice())?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async{let started=Instant::now();let mut stream=UnixStream::connect(socket).await?;if stream.peer_cred()?.uid()!=meta.uid(){return Err("cliwindow.server_uid".into());}let text=Zeroizing::new(if denied{"0".repeat(64)}else{std::str::from_utf8(token.as_slice())?.to_string()});send(&mut stream,&rpc("h","hello",json!({"protocolVersion":1,"token":text.as_str()}))).await?;if denied{let mut b=[0u8;1];if stream.read(&mut b).await?!=0{return Err("cliwindow.denied_receipt".into());}println!("{}",json!({"denied":true}));return Ok(());}let mut d=Decoder::default();let hello:Value=serde_json::from_slice(&read_frame(&mut stream,&mut d).await?)?;if hello["id"]!="h"||hello["result"]["protocolVersion"]!=1{return Err("cliwindow.hello_receipt".into());}let request_started=Instant::now();send(&mut stream,&rpc("w","probe.window",json!({}))).await?;let receipt:Value=serde_json::from_slice(&read_frame(&mut stream,&mut d).await?)?;if receipt["id"]!="w"||receipt["result"]["nativeVisibleRegistered"]!=true{return Err("cliwindow.window_receipt".into());}println!("{}",json!({"connectThroughReceiptUs":micros(started),"requestThroughNativeReceiptUs":micros(request_started),"nativeReceiptMatched":true}));Ok(())})
}
