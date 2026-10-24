//! Win32 clipboard access lives inside the existing native sys boundary.
use crate::clipboard::ClipboardReadError;
use std::{
    mem::{align_of, size_of},
    ptr, slice,
};
use windows_sys::Win32::{
    Foundation::HGLOBAL,
    System::{
        DataExchange::{CloseClipboard, GetClipboardData, OpenClipboard},
        Memory::{GlobalLock, GlobalSize, GlobalUnlock},
    },
};

// Win32's predefined UTF-16 plain-text format; no custom formats or file reads.
const CF_UNICODETEXT: u32 = 13;

struct ClipboardGuard {
    open: bool,
}
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        if self.open {
            // SAFETY: This thread successfully opened the clipboard and still owns it.
            unsafe {
                CloseClipboard();
            }
        }
    }
}
struct LockedMemory(HGLOBAL);
impl Drop for LockedMemory {
    fn drop(&mut self) {
        // SAFETY: This handle had one successful GlobalLock and remains clipboard-owned.
        unsafe {
            GlobalUnlock(self.0);
        }
    }
}

pub(crate) fn read_clipboard_text(
    max_bytes: usize,
    mut authorize: impl FnMut() -> bool,
) -> Result<String, ClipboardReadError> {
    if !authorize() {
        return Err(ClipboardReadError::Denied);
    }
    // SAFETY: Read-only access needs no owner HWND; never empty or set the clipboard.
    if unsafe { OpenClipboard(ptr::null_mut()) } == 0 {
        return Err(ClipboardReadError::Unavailable);
    }
    let mut clipboard = ClipboardGuard { open: true };
    let result = (|| {
        // SAFETY: The clipboard is open on this thread, with a fixed predefined format.
        let handle = unsafe { GetClipboardData(CF_UNICODETEXT) };
        if handle.is_null() {
            return Err(ClipboardReadError::Unavailable);
        }
        // SAFETY: CF_UNICODETEXT supplies a system-owned global memory handle.
        let bytes = unsafe { GlobalSize(handle) };
        let units = (bytes / size_of::<u16>())
            .min(max_bytes.saturating_add(1))
            .min(isize::MAX as usize / size_of::<u16>());
        if units == 0 {
            return Err(ClipboardReadError::Unavailable);
        }
        // SAFETY: Lock the same clipboard-owned handle before accessing its bytes.
        let pointer = unsafe { GlobalLock(handle) };
        if pointer.is_null() {
            return Err(ClipboardReadError::Unavailable);
        }
        let _locked = LockedMemory(handle);
        if !(pointer as usize).is_multiple_of(align_of::<u16>()) {
            return Err(ClipboardReadError::Unavailable);
        }
        // SAFETY: The memory remains locked and clipboard-owned; the aligned slice is
        // bounded by GlobalSize, the caller's budget plus NUL and Rust's slice limit.
        let text = unsafe { slice::from_raw_parts(pointer.cast::<u16>(), units) };
        decode(text, max_bytes)
    })();
    // SAFETY: The clipboard is still open on this thread; the memory lock has ended.
    let closed = unsafe { CloseClipboard() } != 0;
    clipboard.open = false;
    if !closed {
        return Err(ClipboardReadError::Unavailable);
    }
    if !authorize() {
        return Err(ClipboardReadError::Denied);
    }
    result
}

fn decode(units: &[u16], max_bytes: usize) -> Result<String, ClipboardReadError> {
    let end = units
        .iter()
        .position(|&value| value == 0)
        .ok_or(ClipboardReadError::Unavailable)?;
    let text = String::from_utf16(&units[..end]).map_err(|_| ClipboardReadError::Unavailable)?;
    if text.len() > max_bytes {
        return Err(ClipboardReadError::TooLarge);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clipboard_without_original_authorization_never_opens() {
        assert_eq!(
            read_clipboard_text(1012, || false),
            Err(ClipboardReadError::Denied)
        );
    }

    #[test]
    fn unicode_text_requires_terminator_strict_utf16_and_utf8_budget() {
        let text = "ab\r\n中文🙂";
        let units = text.encode_utf16().chain([0, 0xffff]).collect::<Vec<_>>();
        assert_eq!(decode(&units, text.len()), Ok(text.to_owned()));
        assert_eq!(
            decode(&units, text.len() - 1),
            Err(ClipboardReadError::TooLarge)
        );
        assert_eq!(decode(&[0], 0), Ok(String::new()));
        for invalid in [&[0xd800, 0][..], &[0xdc00, 0][..], &[65][..], &[][..]] {
            assert_eq!(decode(invalid, 1012), Err(ClipboardReadError::Unavailable));
        }
    }
}
