//! Existing Linux ownership/mode/peer checks, endpoint identity cleanup and token reads.
use super::{Error, IpcStream, Listener, NativeEndpoint, Result};
use interprocess::{
    local_socket::{GenericFilePath, ListenerOptions, tokio::prelude::*},
    os::unix::local_socket::ListenerOptionsExt,
};
use std::{
    fs::{self, Metadata, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};
use tokio::runtime::Handle;
use zeroize::Zeroizing;
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
pub(super) struct Endpoints {
    socket: Option<Identity>,
    token: Option<Identity>,
}
impl super::NativeEndpoint for Endpoints {
    fn admit_peer(&self, stream: &IpcStream) -> Result<()> {
        validate_peer(stream)
    }
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

pub(super) fn listen(
    directory: &Path,
    runtime: &Handle,
    secret: &[u8; 64],
) -> Result<(Listener, Endpoints)> {
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
    Ok((listener, owned))
}

pub(super) fn validate_peer(stream: &IpcStream) -> Result<()> {
    if stream.peer_creds().ok().and_then(|peer| peer.euid())
        == Some(rustix::process::geteuid().as_raw())
    {
        Ok(())
    } else {
        Err(Error)
    }
}

pub(super) async fn connect(directory: &Path) -> Result<(super::Stream, Zeroizing<Vec<u8>>)> {
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
    let stream = IpcStream::connect(socket.to_fs_name::<GenericFilePath>().map_err(|_| Error)?)
        .await
        .map_err(|_| Error)?;
    if stream.peer_creds().ok().and_then(|peer| peer.euid()) != Some(meta.uid()) {
        return Err(Error);
    }
    Ok((super::Stream(stream), secret))
}
