//! Unix-only, read-only view of a backend-selected `.ssh/known_hosts` file.
use crate::{HostKeyError, HostPin, HostTarget, known_hosts::parse_system_known_hosts};
use rustix::fs::{self as sys, Mode, OFlags};
use std::{
    fs::{self, File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path},
};
const OPEN: OFlags = OFlags::CLOEXEC
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::RDONLY);
const MAX_TEXT: usize = 256 * 1024;

fn identity(m: &Metadata) -> (u64, u64) {
    (m.dev(), m.ino())
}
fn stamp(m: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64) {
    (
        m.dev(),
        m.ino(),
        m.len(),
        m.mtime(),
        m.mtime_nsec(),
        m.ctime(),
        m.ctime_nsec(),
    )
}
fn private_directory(m: &Metadata) -> bool {
    m.is_dir() && m.uid() == rustix::process::geteuid().as_raw() && m.mode() & 0o077 == 0
}
fn trusted_regular(m: &Metadata) -> bool {
    m.is_file()
        && m.uid() == rustix::process::geteuid().as_raw()
        && m.mode() & 0o022 == 0
        && m.nlink() == 1
        && m.len() <= MAX_TEXT as u64
}

/// Read a backend-selected SSH directory without creating files or touching user configuration.
/// A missing `known_hosts` is an empty source; missing/insecure directory or failed file read
/// is `StoreUnavailable`. The caller must not map it to TOFU/Unknown. Leaf links and FIFOs
/// are rejected; this does not provide a complete parent-chain or concurrent-edit guarantee.
pub fn load_system_known_hosts_directory(
    ssh_dir: &Path,
    target: &HostTarget,
) -> Result<Vec<HostPin>, HostKeyError> {
    if !ssh_dir.is_absolute()
        || ssh_dir
            .components()
            .any(|part| matches!(part, Component::ParentDir | Component::CurDir))
    {
        return Err(HostKeyError::StoreUnavailable);
    }
    let directory: File = sys::open(ssh_dir, OPEN | OFlags::DIRECTORY, Mode::empty())
        .map_err(|_| HostKeyError::StoreUnavailable)?
        .into();
    let dir_stat = directory
        .metadata()
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    if !private_directory(&dir_stat) {
        return Err(HostKeyError::StoreUnavailable);
    }
    let file = match sys::openat(&directory, "known_hosts", OPEN, Mode::empty()) {
        Ok(fd) => File::from(fd),
        Err(rustix::io::Errno::NOENT) => {
            let path_stat =
                fs::symlink_metadata(ssh_dir).map_err(|_| HostKeyError::StoreUnavailable)?;
            if !private_directory(&path_stat) || identity(&dir_stat) != identity(&path_stat) {
                return Err(HostKeyError::StoreUnavailable);
            }
            return Ok(Vec::new());
        }
        Err(_) => return Err(HostKeyError::StoreUnavailable),
    };
    let before = file
        .metadata()
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    if !trusted_regular(&before) {
        return Err(HostKeyError::StoreUnavailable);
    }
    let mut bytes = Vec::new();
    (&file)
        .take(MAX_TEXT as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    if bytes.len() > MAX_TEXT {
        return Err(HostKeyError::StoreUnavailable);
    }
    let after = file
        .metadata()
        .map_err(|_| HostKeyError::StoreUnavailable)?;
    let path_stat = fs::symlink_metadata(ssh_dir).map_err(|_| HostKeyError::StoreUnavailable)?;
    if !trusted_regular(&after)
        || stamp(&before) != stamp(&after)
        || !private_directory(&path_stat)
        || identity(&dir_stat) != identity(&path_stat)
    {
        return Err(HostKeyError::StoreUnavailable);
    }
    let text = std::str::from_utf8(&bytes).map_err(|_| HostKeyError::StoreUnavailable)?;
    parse_system_known_hosts(target, text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::verify_host_key;
    use russh::keys::{Algorithm, PrivateKey};
    use std::{
        fs::OpenOptions,
        io::Write,
        os::unix::fs::{OpenOptionsExt, PermissionsExt, symlink},
    };
    type TestResult = Result<(), Box<dyn std::error::Error>>;
    #[test]
    fn owned_regular_file_unknown_key_and_missing_distinguish_read_failures() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".ssh");
        fs::create_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let target = HostTarget::new("example.invalid", 22)?;
        assert_eq!(
            load_system_known_hosts_directory(Path::new(".ssh"), &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        assert_eq!(
            load_system_known_hosts_directory(&dir.join("../.ssh"), &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        assert!(load_system_known_hosts_directory(&dir, &target)?.is_empty());
        assert_eq!(
            load_system_known_hosts_directory(&root.path().join("missing"), &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        let key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)?
            .public_key()
            .clone();
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(dir.join("known_hosts"))?;
        writeln!(file, "example.invalid {}", key.to_openssh()?)?;
        drop(file);
        let pins = load_system_known_hosts_directory(&dir, &target)?;
        assert_eq!(
            verify_host_key(&target, &key, Some(&[]), Some(&pins)),
            Ok(())
        );
        fs::set_permissions(dir.join("known_hosts"), fs::Permissions::from_mode(0o666))?;
        assert_eq!(
            load_system_known_hosts_directory(&dir, &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        Ok(())
    }
    #[test]
    fn links_fifo_insecure_dir_and_oversize_fail_without_writes_or_waiting() -> TestResult {
        let root = tempfile::tempdir()?;
        let dir = root.path().join(".ssh");
        fs::create_dir(&dir)?;
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))?;
        let target = HostTarget::new("host.invalid", 22)?;
        symlink(&dir, root.path().join("redirect"))?;
        assert_eq!(
            load_system_known_hosts_directory(&root.path().join("redirect"), &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        symlink("../other", dir.join("known_hosts"))?;
        assert_eq!(
            load_system_known_hosts_directory(&dir, &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        fs::remove_file(dir.join("known_hosts"))?;
        #[cfg(not(any(target_vendor = "apple", target_os = "redox", target_os = "wasi")))]
        {
            sys::mkfifoat(sys::CWD, dir.join("known_hosts"), Mode::RUSR | Mode::WUSR)?;
            assert_eq!(
                load_system_known_hosts_directory(&dir, &target).err(),
                Some(HostKeyError::StoreUnavailable)
            );
            fs::remove_file(dir.join("known_hosts"))?;
        }
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(dir.join("known_hosts"))?;
        file.write_all(&vec![b'x'; MAX_TEXT + 1])?;
        drop(file);
        assert_eq!(
            load_system_known_hosts_directory(&dir, &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        fs::set_permissions(&dir, fs::Permissions::from_mode(0o777))?;
        assert_eq!(
            load_system_known_hosts_directory(&dir, &target).err(),
            Some(HostKeyError::StoreUnavailable)
        );
        Ok(())
    }
}
