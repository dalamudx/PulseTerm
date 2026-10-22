//! Windows native control adapter shared verbatim by the GUI/CLI and the opt-in native probe.
//! Filesystem/OS admission belongs here; pulse-app owns only the bounded protocol and supervision.
use interprocess::{
    local_socket::{
        GenericFilePath, ListenerOptions,
        tokio::{Stream as IpcStream, prelude::*},
    },
    os::windows::{local_socket::ListenerOptionsExt, security_descriptor::SecurityDescriptor},
};
use pulse_app::cli_control::{
    Client, Error, Handler, NativeEndpoint, Result, Server, TransportToken,
};
use pulse_platform::windows::{LocalControlToken, PrivateDirectory, process_is_current_user};
use std::{path::Path, sync::Arc, time::Duration};
use tokio::runtime::Handle;
use widestring::U16CString;
use zeroize::Zeroizing;

struct Endpoints {
    token: Option<LocalControlToken>,
}
impl NativeEndpoint for Endpoints {
    fn admit_peer(&self, stream: &IpcStream) -> Result<()> {
        admit_pid(stream.peer_creds().map_err(|_| Error)?.pid().ok_or(Error)?)
    }
    fn cleanup(&mut self) -> Result<()> {
        match self.token.take() {
            Some(mut token) => token.cleanup().map_err(|_| Error),
            None => Ok(()),
        }
    }
}
impl Drop for Endpoints {
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
fn admit_pid(pid: u32) -> Result<()> {
    if process_is_current_user(pid).map_err(|_| Error)? {
        Ok(())
    } else {
        Err(Error)
    }
}
/// Publish one private native endpoint; retained leases outlive cancelled waiters and all handlers.
/// The caller supplies the backend-selected directory. Occupied/stale endpoints are never repaired.
pub fn start(directory: &Path, runtime: &Handle, handler: Arc<dyn Handler>) -> Result<Server> {
    let directory = PrivateDirectory::prepare(directory).map_err(|_| Error)?;
    let identity = directory.local_control_identity().map_err(|_| Error)?;
    let sddl = U16CString::from_str(identity.security_descriptor()).map_err(|_| Error)?;
    let descriptor = SecurityDescriptor::deserialize(&sddl).map_err(|_| Error)?;
    let name = identity
        .pipe_name()
        .to_fs_name::<GenericFilePath>()
        .map_err(|_| Error)?;
    let listener = {
        let _entered = runtime.enter();
        ListenerOptions::new()
            .name(name)
            .security_descriptor(descriptor)
            .reclaim_name(false)
            .try_overwrite(false)
            .create_tokio()
            .map_err(|_| Error)?
    };
    let secret = TransportToken::generate();
    let token = LocalControlToken::create(directory, secret.bytes()).map_err(|_| Error)?;
    Ok(Server::start_admitted(
        listener,
        secret,
        Box::new(Endpoints { token: Some(token) }),
        runtime,
        handler,
    ))
}
/// Warm only: authenticate the fixed local pipe derived from the original admitted private directory.
/// Missing/unsafe endpoints fail; no GUI is launched and no stale file is deleted. Five seconds total.
pub async fn connect(directory: &Path) -> Result<Client> {
    tokio::time::timeout(Duration::from_secs(5), async {
        let directory = PrivateDirectory::open(directory).map_err(|_| Error)?;
        let identity = directory.local_control_identity().map_err(|_| Error)?;
        let secret = Zeroizing::new(
            directory
                .open_file("control.token")
                .map_err(|_| Error)?
                .read_bounded(64)
                .map_err(|_| Error)?,
        );
        if secret.len() != 64
            || !secret
                .iter()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(b))
        {
            return Err(Error);
        }
        let handle = loop {
            match identity.open_client_handle() {
                Ok(handle) => break handle,
                Err(error) if error.raw_os_error() == Some(231) => {
                    tokio::time::sleep(Duration::from_millis(10)).await
                }
                Err(_) => return Err(Error),
            }
        };
        Client::connect_native(handle, secret, admit_pid).await
    })
    .await
    .map_err(|_| Error)?
}
#[cfg(test)]
#[path = "native_cli_windows_tests.rs"]
mod tests;
