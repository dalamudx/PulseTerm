//! Bounded fixed OS metadata queries, kept under the platform sys FFI boundary.
use crate::{DirectoryError::Location, DirectoryResult};
use std::{
    ffi::OsStr,
    os::windows::ffi::{OsStrExt, OsStringExt},
    path::PathBuf,
};
use windows_sys::Win32::System::{
    Registry::{HKEY_CURRENT_USER, RRF_RT_REG_SZ, RegGetValueW},
    SystemInformation::GetSystemDirectoryW,
};

pub(crate) fn system_directory() -> DirectoryResult<PathBuf> {
    let mut buffer = [0u16; 4096];
    // SAFETY: writable buffer has exactly the length passed to the OS.
    let len = unsafe { GetSystemDirectoryW(buffer.as_mut_ptr(), buffer.len() as u32) } as usize;
    if len == 0 || len >= buffer.len() || buffer[..len].contains(&0) {
        return Err(Location);
    }
    let path = PathBuf::from(std::ffi::OsString::from_wide(&buffer[..len]));
    if !path.is_absolute() {
        return Err(Location);
    }
    Ok(path)
}

fn registry_string(key: &str, name: &str) -> DirectoryResult<Option<String>> {
    let key: Vec<u16> = OsStr::new(key).encode_wide().chain([0]).collect();
    let name: Vec<u16> = OsStr::new(name).encode_wide().chain([0]).collect();
    let mut buffer = [0u16; 1024];
    let mut bytes = (buffer.len() * 2) as u32;
    // SAFETY: fixed root and bounded, NUL-terminated names; output size matches the writable buffer.
    // REG_SZ only: no environment expansion and no environment-key enumeration or logging.
    let status = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            key.as_ptr(),
            name.as_ptr(),
            RRF_RT_REG_SZ,
            std::ptr::null_mut(),
            buffer.as_mut_ptr().cast(),
            &mut bytes,
        )
    };
    if status == 2 || status == 3 {
        return Ok(None);
    }
    if status != 0 || bytes < 2 || bytes as usize > buffer.len() * 2 || !bytes.is_multiple_of(2) {
        return Err(Location);
    }
    let value = &buffer[..bytes as usize / 2];
    let value = value.strip_suffix(&[0]).ok_or(Location)?;
    if value.contains(&0) {
        return Err(Location);
    }
    Ok(Some(String::from_utf16(value).map_err(|_| Location)?))
}

pub(crate) fn default_wsl_distribution() -> DirectoryResult<Option<String>> {
    const ROOT: &str = r"Software\Microsoft\Windows\CurrentVersion\Lxss";
    let Some(guid) = registry_string(ROOT, "DefaultDistribution")? else {
        return Ok(None);
    };
    let bytes = guid.as_bytes();
    if bytes.len() != 38
        || bytes[0] != b'{'
        || bytes[37] != b'}'
        || !bytes[1..37].iter().enumerate().all(|(i, b)| {
            if matches!(i, 8 | 13 | 18 | 23) {
                *b == b'-'
            } else {
                b.is_ascii_hexdigit()
            }
        })
    {
        return Err(Location);
    }
    let name = registry_string(&format!("{ROOT}\\{guid}"), "DistributionName")?.ok_or(Location)?;
    if name.is_empty() || name.len() > 256 || name.chars().any(char::is_control) {
        return Err(Location);
    }
    Ok(Some(name))
}
