use crate::web_link::WebLinkError;
use windows_sys::Win32::{
    System::Com::{COINIT_APARTMENTTHREADED, CoInitializeEx, CoUninitialize},
    UI::{
        Shell::{SEE_MASK_FLAG_NO_UI, SEE_MASK_NOASYNC, SHELLEXECUTEINFOW, ShellExecuteExW},
        WindowsAndMessaging::SW_SHOWNORMAL,
    },
};

struct Apartment;
impl Drop for Apartment {
    fn drop(&mut self) {
        // SAFETY: this guard exists only after successful initialization on this same thread.
        unsafe { CoUninitialize() };
    }
}

pub(in crate::windows) fn open_web_url(
    url: &str,
    authorize: impl FnOnce() -> bool,
) -> Result<(), WebLinkError> {
    // SAFETY: reserved is null; callers use a dedicated thread and the guard balances success.
    if unsafe { CoInitializeEx(std::ptr::null(), COINIT_APARTMENTTHREADED as u32) } < 0 {
        return Err(WebLinkError::Failed);
    }
    let _apartment = Apartment;
    let url: Vec<u16> = url.encode_utf16().chain([0]).collect();
    let verb: Vec<u16> = "open".encode_utf16().chain([0]).collect();
    let mut request = SHELLEXECUTEINFOW {
        cbSize: std::mem::size_of::<SHELLEXECUTEINFOW>() as u32,
        fMask: SEE_MASK_NOASYNC | SEE_MASK_FLAG_NO_UI,
        lpVerb: verb.as_ptr(),
        lpFile: url.as_ptr(),
        nShow: SW_SHOWNORMAL,
        ..Default::default()
    };
    if !authorize() {
        return Err(WebLinkError::Denied);
    }
    // SAFETY: initialized struct and terminated strings live through the synchronous dispatch.
    // No parameters, executable selection, process handle request or zone-check bypass is supplied.
    if unsafe { ShellExecuteExW(&mut request) } == 0 {
        return Err(WebLinkError::Failed);
    }
    Ok(())
}
