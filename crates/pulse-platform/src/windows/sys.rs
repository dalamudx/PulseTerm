use crate::{DirectoryError::Location, DirectoryResult, policy};
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
        BY_HANDLE_FILE_INFORMATION, CreateDirectoryW, FILE_ID_INFO, FileIdInfo, GetDriveTypeW,
        GetFileInformationByHandle, GetFileInformationByHandleEx, GetVolumeInformationByHandleW,
    },
    System::Threading::{GetCurrentProcess, OpenProcessToken},
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct Identity {
    volume: u64,
    index: [u8; 16],
}
pub(super) struct Facts {
    pub identity: Identity,
    pub attributes: u32,
    pub owner: Vec<u8>,
    pub acl: Option<Vec<u8>>,
    pub protected: bool,
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
        owner,
        acl,
        protected: control & 0x1000 != 0,
    })
}

fn sid_text(sid: &[u8]) -> DirectoryResult<String> {
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

fn create_sddl(path: &Path, sddl: &str) -> DirectoryResult<()> {
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
    let sd = Local(raw);
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
