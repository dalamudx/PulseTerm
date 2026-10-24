#[path = "clipboard.rs"]
mod clipboard;
pub(super) use clipboard::read_clipboard_text;
#[path = "shell_metadata.rs"]
mod shell_metadata;
use crate::{DirectoryError::Location, DirectoryResult, policy};
pub(super) use shell_metadata::{default_wsl_distribution, system_directory};
#[path = "web_link.rs"]
mod web_link;
use std::{
    ffi::c_void,
    fs::File,
    os::windows::{
        ffi::OsStrExt,
        io::{AsRawHandle, FromRawHandle, OwnedHandle},
    },
    path::Path,
    ptr, slice,
};
pub(super) use web_link::open_web_url;
#[path = "local_control_native.rs"]
mod local_control_native;
pub(super) use local_control_native::{
    control_pipe_present, local_application_data_directory, open_control_client,
    open_control_token_for_delete, process_is_current_user, verify_single_instance_owner,
};
use windows_sys::Win32::{
    Foundation::{ERROR_ALREADY_EXISTS, ERROR_INSUFFICIENT_BUFFER, GetLastError, LocalFree},
    Security::{
        Authorization::{
            ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SE_FILE_OBJECT,
        },
        DACL_SECURITY_INFORMATION, GetSecurityDescriptorControl, GetSecurityDescriptorDacl,
        GetSecurityDescriptorLength, GetSecurityDescriptorOwner, GetTokenInformation,
        IsValidSecurityDescriptor, OWNER_SECURITY_INFORMATION, SECURITY_ATTRIBUTES, TOKEN_QUERY,
        TOKEN_USER, TokenUser,
    },
    Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, CreateFileW, FILE_BASIC_INFO,
        FILE_DISPOSITION_INFO, FILE_ID_INFO, FILE_RENAME_INFO, FileBasicInfo, FileDispositionInfo,
        FileIdInfo, FileRenameInfo, GetDriveTypeW, GetFileInformationByHandle,
        GetFileInformationByHandleEx, GetVolumeInformationByHandleW, SetFileInformationByHandle,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

pub(super) use crate::file_state::Identity;
pub(super) struct Facts {
    pub identity: Identity,
    pub attributes: u32,
    pub links: u32,
    pub length: u64,
    pub last_write: u64,
    pub changed: i64,
    pub created: i64,
    pub owner: Vec<u8>,
    pub acl: Option<Vec<u8>>,
    pub protected: bool,
}
impl Facts {
    pub(super) fn stamp(&self) -> crate::file_state::FileStamp {
        crate::file_state::FileStamp {
            identity: self.identity,
            length: self.length,
            written: self.last_write,
            changed: self.changed,
            created: self.created,
        }
    }
}
struct Local(*mut c_void);
impl Drop for Local {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // SAFETY: Only successful native LocalAlloc-family outputs enter this guard; it owns one allocation.
            unsafe {
                LocalFree(self.0);
            }
        }
    }
}

fn region(bytes: &[u8], base: usize, pointer: usize) -> DirectoryResult<&[u8]> {
    let offset = pointer.checked_sub(base).ok_or(Location)?;
    bytes.get(offset..).ok_or(Location)
}

pub(super) fn user() -> DirectoryResult<Vec<u8>> {
    let mut raw = ptr::null_mut();
    // SAFETY: The pseudo process handle is valid; raw is writable, and TOKEN_QUERY grants read-only inspection.
    if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut raw) } == 0 {
        return Err(Location);
    }
    // SAFETY: Successful OpenProcessToken returns a newly owned non-pseudo token handle.
    let token = unsafe { OwnedHandle::from_raw_handle(raw) };
    token_user(&token)
}

fn token_user(token: &OwnedHandle) -> DirectoryResult<Vec<u8>> {
    let mut size = 0;
    // SAFETY: A null zero-size output requests only the required buffer length from this owned token.
    let first = unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            ptr::null_mut(),
            0,
            &mut size,
        )
    };
    // SAFETY: GetLastError immediately follows the failed size query, without an intervening native call.
    if first != 0
        || unsafe { GetLastError() } != ERROR_INSUFFICIENT_BUFFER
        || size < size_of::<TOKEN_USER>() as u32
        || size > 65536
    {
        return Err(Location);
    }
    let mut storage = vec![0usize; (size as usize).div_ceil(size_of::<usize>())];
    let capacity = size;
    // SAFETY: Initialized usize storage has TOKEN_USER alignment and at least the requested writable byte capacity.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle(),
            TokenUser,
            storage.as_mut_ptr().cast(),
            capacity,
            &mut size,
        )
    } == 0
        || size > capacity
    {
        return Err(Location);
    }
    // SAFETY: The successful TokenUser query initialized its fixed header in aligned, live storage.
    let info = unsafe { ptr::read(storage.as_ptr().cast::<TOKEN_USER>()) };
    // SAFETY: Storage is initialized and the native result length was checked against its allocated byte capacity.
    let bytes = unsafe { slice::from_raw_parts(storage.as_ptr().cast::<u8>(), size as usize) };
    Ok(policy::sid(region(
        bytes,
        storage.as_ptr() as usize,
        info.User.Sid as usize,
    )?)?
    .to_vec())
}

pub(super) fn local_drive(drive: u8) -> DirectoryResult<()> {
    let root = [u16::from(drive), 58, 92, 0];
    // SAFETY: The fixed local-drive root is a valid NUL-terminated UTF-16 string.
    if unsafe { GetDriveTypeW(root.as_ptr()) } != 3 {
        return Err(Location);
    } // DRIVE_FIXED; no mapped/remote volumes.
    Ok(())
}

pub(super) fn inspect(file: &File) -> DirectoryResult<Facts> {
    let mut meta = BY_HANDLE_FILE_INFORMATION::default();
    let mut flags = 0;
    // SAFETY: File owns a live handle and both native output structures have correctly sized writable storage.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &mut meta) } == 0 {
        return Err(Location);
    }
    let mut basic = FILE_BASIC_INFO::default();
    // SAFETY: File is live; FileBasicInfo writes one correctly sized, aligned initialized structure.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileBasicInfo,
            ptr::from_mut(&mut basic).cast(),
            size_of::<FILE_BASIC_INFO>() as u32,
        )
    } == 0
    {
        return Err(Location);
    }
    let mut file_id = FILE_ID_INFO::default();
    // SAFETY: File is live; FileIdInfo writes exactly the correctly sized/aligned FILE_ID_INFO output.
    if unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            ptr::from_mut(&mut file_id).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    } == 0
    {
        return Err(Location);
    }
    // SAFETY: Optional name/identity outputs are null with length zero; flags points to one writable DWORD.
    if unsafe {
        GetVolumeInformationByHandleW(
            file.as_raw_handle(),
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut flags,
            ptr::null_mut(),
            0,
        )
    } == 0
        || flags & 8 == 0
    {
        return Err(Location);
    } // FILE_PERSISTENT_ACLS.
    let mut raw = ptr::null_mut();
    // SAFETY: File is live; optional component outputs are omitted, while raw receives an owned self-relative descriptor.
    let code = unsafe {
        GetSecurityInfo(
            file.as_raw_handle(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            ptr::null_mut(),
            &mut raw,
        )
    };
    if code != 0 || raw.is_null() {
        return Err(Location);
    }
    let sd = Local(raw);
    // SAFETY: A successful GetSecurityInfo allocated the descriptor, retained by sd until all borrows end.
    if unsafe { IsValidSecurityDescriptor(sd.0) } == 0 {
        return Err(Location);
    }
    let mut control = 0;
    let mut revision = 0;
    // SAFETY: sd is a validated live descriptor; both outputs have the documented scalar sizes.
    if unsafe { GetSecurityDescriptorControl(sd.0, &mut control, &mut revision) } == 0
        || control & 0x8000 == 0
    {
        return Err(Location);
    }
    // SAFETY: The validated self-relative descriptor reports the extent of its own native allocation.
    let length = unsafe { GetSecurityDescriptorLength(sd.0) } as usize;
    if !(20..=1048576).contains(&length) {
        return Err(Location);
    }
    // SAFETY: The allocation is self-relative and validated, with its checked native extent and a live owning guard.
    let bytes = unsafe { slice::from_raw_parts(sd.0.cast::<u8>(), length) };
    let mut owner = ptr::null_mut();
    let mut defaulted = 0;
    // SAFETY: Component pointers are borrowed from the live validated self-relative allocation, never freed separately.
    if unsafe { GetSecurityDescriptorOwner(sd.0, &mut owner, &mut defaulted) } == 0
        || owner.is_null()
    {
        return Err(Location);
    }
    let owner = policy::sid(region(bytes, sd.0 as usize, owner as usize)?)?.to_vec();
    let mut acl = ptr::null_mut();
    let mut present = 0;
    // SAFETY: The live descriptor owns the optional DACL; outputs are valid writable pointer/scalar slots.
    if unsafe { GetSecurityDescriptorDacl(sd.0, &mut present, &mut acl, &mut defaulted) } == 0 {
        return Err(Location);
    }
    let acl = if present == 0 || acl.is_null() {
        None
    } else {
        let tail = region(bytes, sd.0 as usize, acl as usize)?;
        if tail.len() < 8 {
            return Err(Location);
        }
        let len = usize::from(u16::from_le_bytes([tail[2], tail[3]]));
        Some(tail.get(..len).ok_or(Location)?.to_vec())
    };
    Ok(Facts {
        identity: Identity {
            volume: file_id.VolumeSerialNumber,
            index: file_id.FileId.Identifier,
        },
        attributes: meta.dwFileAttributes,
        links: meta.nNumberOfLinks,
        length: (u64::from(meta.nFileSizeHigh) << 32) | u64::from(meta.nFileSizeLow),
        last_write: (u64::from(meta.ftLastWriteTime.dwHighDateTime) << 32)
            | u64::from(meta.ftLastWriteTime.dwLowDateTime),
        changed: basic.ChangeTime,
        created: basic.CreationTime,
        owner,
        acl,
        protected: control & 0x1000 != 0,
    })
}

pub(super) fn sid_text(sid: &[u8]) -> DirectoryResult<String> {
    let sid = policy::sid(sid)?;
    let authority = sid[2..8].iter().fold(0u64, |n, b| (n << 8) | u64::from(*b));
    let mut s = format!("S-1-{authority}");
    for c in sid[8..].chunks_exact(4) {
        s.push_str(&format!(
            "-{}",
            u32::from_le_bytes([c[0], c[1], c[2], c[3]])
        ));
    }
    Ok(s)
}

fn descriptor(sddl: &str) -> DirectoryResult<Local> {
    let text: Vec<u16> = sddl.encode_utf16().chain([0]).collect();
    let mut raw = ptr::null_mut();
    // SAFETY: text is terminated UTF-16; raw receives one LocalAlloc-owned descriptor on success.
    if unsafe {
        ConvertStringSecurityDescriptorToSecurityDescriptorW(
            text.as_ptr(),
            1,
            &mut raw,
            ptr::null_mut(),
        )
    } == 0
        || raw.is_null()
    {
        return Err(Location);
    }
    Ok(Local(raw))
}

fn create_sddl(path: &Path, sddl: &str) -> DirectoryResult<()> {
    let sd = descriptor(sddl)?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.0,
        bInheritHandle: 0,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    // SAFETY: Validated terminated path and security attributes borrow live buffers for this synchronous creation only.
    let ok = unsafe { CreateDirectoryW(path.as_ptr(), &attrs) };
    // SAFETY: GetLastError immediately follows failed creation; existing objects are only admitted, never modified.
    if ok == 0 && unsafe { GetLastError() } != ERROR_ALREADY_EXISTS {
        return Err(Location);
    }
    Ok(())
}

pub(super) fn create_private(path: &Path, user: &[u8]) -> DirectoryResult<()> {
    let user = sid_text(user)?;
    create_sddl(
        path,
        &format!("O:{user}D:P(A;OICI;FA;;;{user})(A;OICI;FA;;;SY)(A;OICI;FA;;;BA)"),
    )
}

#[cfg(test)]
pub(super) fn create_broad_fixture(path: &Path, inherit_only: bool) -> DirectoryResult<()> {
    let user = sid_text(&self::user()?)?;
    let flags = if inherit_only { "OICIIO" } else { "OICI" };
    create_sddl(
        path,
        &format!("O:{user}D:P(A;OICI;FA;;;{user})(A;{flags};GR;;;WD)"),
    )
}

fn regular_with_sddl(path: &Path, sddl: Option<&str>, exclusive: bool) -> std::io::Result<File> {
    let sd = sddl
        .map(descriptor)
        .transpose()
        .map_err(|_| std::io::Error::from_raw_os_error(5))?;
    let attrs = SECURITY_ATTRIBUTES {
        nLength: size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor: sd.as_ref().map_or(ptr::null_mut(), |d| d.0),
        bInheritHandle: 0,
    };
    let path: Vec<u16> = path.as_os_str().encode_wide().chain([0]).collect();
    let create = sd.is_some();
    // GENERIC_READ | GENERIC_WRITE | READ_CONTROL | DELETE, no sharing, CREATE_NEW.
    // Existing readers use GENERIC_READ | READ_CONTROL, FILE_SHARE_READ and OPEN_EXISTING.
    let (access, share, disposition) = if create {
        (0xc0030000, 0, 1)
    } else if exclusive {
        (0xc0020000, 0, 3) // The cooperative lock: read/write, no sharing, OPEN_EXISTING.
    } else {
        (0x80020000, 1, 3)
    };
    // SAFETY: Path is a validated terminated basename under a pinned directory; attributes borrow a live owned descriptor.
    // CREATE_NEW never truncates; existing opens omit write/delete sharing and never follow a final reparse point.
    let raw = unsafe {
        CreateFileW(
            path.as_ptr(),
            access,
            share,
            if create { &attrs } else { ptr::null() },
            disposition,
            0x00200080,
            ptr::null_mut(),
        )
    };
    if raw == windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: Successful CreateFileW transferred one owned, non-inheritable file handle.
    Ok(unsafe { File::from_raw_handle(raw) })
}

pub(super) fn open_regular(path: &Path, create_user: Option<&[u8]>) -> std::io::Result<File> {
    let sddl = create_user
        .map(|user| {
            let user = sid_text(user)?;
            Ok::<_, crate::DirectoryError>(format!(
                "O:{user}D:P(A;;FA;;;{user})(A;;FA;;;SY)(A;;FA;;;BA)"
            ))
        })
        .transpose()
        .map_err(|_| std::io::Error::from_raw_os_error(5))?;
    regular_with_sddl(path, sddl.as_deref(), false)
}

pub(super) fn discard_file(file: &File) -> std::io::Result<()> {
    let info = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: The creator owns DELETE access and exclusive sharing; the sized native structure is live for this synchronous call.
    // This marks only that exact file for deletion on handle close, never a later replacement at its old path.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            ptr::from_ref(&info).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}

#[cfg(test)]
pub(super) fn create_broad_file_fixture(path: &Path) -> DirectoryResult<()> {
    let user = sid_text(&self::user()?)?;
    regular_with_sddl(
        path,
        Some(&format!("O:{user}D:P(A;;FA;;;{user})(A;;GR;;;WD)")),
        false,
    )
    .map_err(|_| Location)?;
    Ok(())
}

pub(super) fn open_lock(path: &Path) -> std::io::Result<File> {
    regular_with_sddl(path, None, true)
}

pub(super) struct RenameRequest {
    storage: Vec<usize>,
    length: u32,
}
pub(super) fn prepare_rename(path: &Path, replace: bool) -> std::io::Result<RenameRequest> {
    let mut name: Vec<u16> = path.as_os_str().encode_wide().take(32768).collect();
    if name.is_empty() || name.len() > 32767 || name.contains(&0) {
        return Err(std::io::Error::from_raw_os_error(87));
    }
    let byte_length = name.len() * 2;
    name.push(0);
    let offset = std::mem::offset_of!(FILE_RENAME_INFO, FileName);
    let length = size_of::<FILE_RENAME_INFO>().max(offset + name.len() * 2);
    let mut storage = vec![0usize; length.div_ceil(size_of::<usize>())];
    let header = storage.as_mut_ptr().cast::<FILE_RENAME_INFO>();
    // SAFETY: usize allocation provides FILE_RENAME_INFO alignment; all bytes start initialized.
    // Field writes preserve initialized padding. The bounded UTF-16 tail fits the checked allocation.
    unsafe {
        ptr::addr_of_mut!((*header).Anonymous.Flags).write(u32::from(replace));
        ptr::addr_of_mut!((*header).RootDirectory).write(ptr::null_mut());
        ptr::addr_of_mut!((*header).FileNameLength).write(byte_length as u32);
        ptr::copy_nonoverlapping(
            name.as_ptr(),
            storage.as_mut_ptr().cast::<u8>().add(offset).cast::<u16>(),
            name.len(),
        );
    }
    Ok(RenameRequest {
        storage,
        length: length as u32,
    })
}
pub(super) fn rename_file(file: &File, request: &RenameRequest) -> std::io::Result<()> {
    // SAFETY: The staged file owns DELETE access. Request owns aligned initialized FILE_RENAME_INFO
    // plus its bounded terminated name; RootDirectory is null and the pinned-parent path is absolute.
    if unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileRenameInfo,
            request.storage.as_ptr().cast(),
            request.length,
        )
    } == 0
    {
        return Err(std::io::Error::last_os_error());
    }
    Ok(())
}
