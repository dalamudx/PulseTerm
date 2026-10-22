//! Native local-control process identity and nonblocking, non-impersonable pipe open.
use super::*;
use std::os::windows::io::{IntoRawHandle, OwnedHandle};
use windows_sys::Win32::System::Threading::{OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION};

pub(in super::super) fn local_application_data_directory() -> DirectoryResult<std::path::PathBuf> {
    use std::os::windows::ffi::OsStringExt;
    use windows_sys::Win32::{
        System::Com::CoTaskMemFree,
        UI::Shell::{FOLDERID_LocalAppData, SHGetKnownFolderPath},
    };
    struct Folder(*mut u16);
    impl Drop for Folder {
        fn drop(&mut self) {
            // SAFETY: SHGetKnownFolderPath returns CoTaskMem-owned memory; null is accepted.
            unsafe {
                CoTaskMemFree(self.0.cast());
            }
        }
    }
    let mut raw = ptr::null_mut();
    // SAFETY: The fixed KnownFolder ID and null current-user token are valid, with writable output.
    let result =
        unsafe { SHGetKnownFolderPath(&FOLDERID_LocalAppData, 0, ptr::null_mut(), &mut raw) };
    let folder = Folder(raw);
    if result < 0 || folder.0.is_null() {
        return Err(Location);
    }
    // SAFETY: Successful SHGetKnownFolderPath returns a NUL-terminated UTF-16 path, live through Folder.
    let length = (0..32768)
        .find(|&index| unsafe { *folder.0.add(index) == 0 })
        .ok_or(Location)?;
    // SAFETY: The bounded prefix above ends at the API's guaranteed terminator.
    let wide = unsafe { slice::from_raw_parts(folder.0, length) };
    Ok(std::ffi::OsString::from_wide(wide).into())
}

pub(in super::super) fn verify_single_instance_owner(identifier: &str) -> DirectoryResult<()> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{FindWindowW, GetWindowThreadProcessId};
    if identifier.is_empty()
        || identifier.len() > 128
        || !identifier
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-_".contains(&b))
    {
        return Err(Location);
    }
    // Names match locked tauri-plugin-single-instance 2.5.3 without its optional semver feature.
    let class: Vec<u16> = format!("{identifier}-sic")
        .encode_utf16()
        .chain([0])
        .collect();
    let name: Vec<u16> = format!("{identifier}-siw")
        .encode_utf16()
        .chain([0])
        .collect();
    // SAFETY: Exact fixed-format class/title strings are terminated and live for this metadata query.
    let window = unsafe { FindWindowW(class.as_ptr(), name.as_ptr()) };
    if window.is_null() {
        return Err(Location);
    }
    let mut pid = 0;
    // SAFETY: Read-only query; pid is writable. A destroyed/unavailable window returns zero.
    let thread = unsafe { GetWindowThreadProcessId(window, &mut pid) };
    if thread == 0 || pid != std::process::id() {
        return Err(Location);
    }
    Ok(())
}
pub(in super::super) fn process_is_current_user(pid: u32) -> DirectoryResult<bool> {
    if pid == 0 {
        return Err(Location);
    }
    // SAFETY: Read-only query access; pid is metadata obtained from the connected pipe.
    let process = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if process.is_null() {
        return Err(Location);
    }
    // SAFETY: Successful OpenProcess transferred a uniquely owned handle.
    let process = unsafe { OwnedHandle::from_raw_handle(process) };
    let mut token = ptr::null_mut();
    // SAFETY: The process handle is live and token is a writable output for one owned handle.
    if unsafe { OpenProcessToken(process.as_raw_handle(), TOKEN_QUERY, &mut token) } == 0 {
        return Err(Location);
    }
    // SAFETY: Successful OpenProcessToken transferred a newly owned non-pseudo handle.
    let token = unsafe { OwnedHandle::from_raw_handle(token) };
    Ok(super::token_user(&token)? == super::user()?)
}

pub(in super::super) fn control_pipe_present(name: &str) -> DirectoryResult<bool> {
    use windows_sys::Win32::System::Pipes::WaitNamedPipeW;
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: Only the admitted local-control identity supplies the terminated local pipe name.
    // The one-millisecond wait never connects to a server or starts background I/O.
    if unsafe { WaitNamedPipeW(name.as_ptr(), 1) } != 0 {
        return Ok(true);
    }
    match std::io::Error::last_os_error().raw_os_error() {
        Some(2) => Ok(false),        // ERROR_FILE_NOT_FOUND.
        Some(121 | 231) => Ok(true), // Existing but busy: ERROR_SEM_TIMEOUT / ERROR_PIPE_BUSY.
        _ => Err(Location),
    }
}

pub(in super::super) fn open_control_client(name: &str) -> std::io::Result<OwnedHandle> {
    let name: Vec<u16> = name.encode_utf16().chain([0]).collect();
    // SAFETY: Only the local-control identity builds name; it is a terminated local pipe path.
    // OVERLAPPED prevents blocking I/O. IDENTIFICATION | SQOS_PRESENT forbids server impersonation.
    let handle = unsafe {
        CreateFileW(
            name.as_ptr(),
            0xc0000000,
            3,
            ptr::null(),
            3,
            0x40000000 | 0x00100000 | 0x00010000,
            ptr::null_mut(),
        )
    };
    if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: Successful CreateFileW transferred one owned non-inheritable handle.
    Ok(unsafe { OwnedHandle::from_raw_handle(handle) })
}

pub(in super::super) fn open_control_token_for_delete(path: &Path) -> std::io::Result<File> {
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: The caller validates the held private parent. OPEN_EXISTING never creates/truncates,
    // OPEN_REPARSE_POINT prevents following the leaf. READ_CONTROL | GENERIC_READ | DELETE, no sharing.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            0x80030000,
            0,
            ptr::null(),
            3,
            0x00200080,
            ptr::null_mut(),
        )
    };
    if handle == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: Successful CreateFileW transferred one uniquely owned handle.
    let handle = unsafe { OwnedHandle::from_raw_handle(handle) };
    // SAFETY: Ownership is transferred exactly once into File.
    Ok(unsafe { File::from_raw_handle(handle.into_raw_handle()) })
}
