//! Plain-text AppKit reads; callers retain native credit and original-view authorization.
use objc2::MainThreadMarker;
use objc2_app_kit::NSPasteboard;
use objc2_foundation::NSString;

pub use crate::clipboard::ClipboardReadError;

/// Read only general UTF-8 plain text on the main thread after authorization.
/// Recheck authorization before returning; the native call itself is not cancellable.
/// Native data may already be allocated by AppKit; check its length before copying.
pub fn read_clipboard_text(
    max_bytes: usize,
    authorize: impl FnMut() -> bool,
) -> Result<String, ClipboardReadError> {
    let _main = MainThreadMarker::new().ok_or(ClipboardReadError::Unavailable)?;
    read_board(&NSPasteboard::generalPasteboard(), max_bytes, authorize)
}

fn read_board(
    board: &NSPasteboard,
    max_bytes: usize,
    mut authorize: impl FnMut() -> bool,
) -> Result<String, ClipboardReadError> {
    if !authorize() {
        return Err(ClipboardReadError::Denied);
    }
    let data = board
        .dataForType(&NSString::from_str("public.utf8-plain-text"))
        .ok_or(ClipboardReadError::Unavailable)?;
    if data.len() > max_bytes {
        return Err(ClipboardReadError::TooLarge);
    }
    let text = String::from_utf8(data.to_vec()).map_err(|_| ClipboardReadError::Unavailable)?;
    if !authorize() {
        return Err(ClipboardReadError::Denied);
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use objc2::rc::Retained;
    use objc2_foundation::NSData;

    fn private_board(bytes: &[u8]) -> Retained<NSPasteboard> {
        let board = NSPasteboard::pasteboardWithUniqueName();
        let _ = board.clearContents();
        assert!(board.setData_forType(
            Some(&NSData::with_bytes(bytes)),
            &NSString::from_str("public.utf8-plain-text"),
        ));
        board
    }

    #[test]
    fn private_pasteboard_preserves_utf8_budget_and_original_authorization() {
        // Use an isolated named board, never the user's general clipboard.
        let text = "中文🙂";
        let board = private_board(text.as_bytes());
        let mut checks = 0;
        assert_eq!(
            read_board(&board, text.len(), || {
                checks += 1;
                true
            }),
            Ok(text.to_owned())
        );
        assert_eq!(checks, 2);
        assert_eq!(
            read_board(&board, text.len() - 1, || true),
            Err(ClipboardReadError::TooLarge)
        );
        assert_eq!(
            read_board(&board, text.len(), || false),
            Err(ClipboardReadError::Denied)
        );
        let mut checks = 0;
        assert_eq!(
            read_board(&board, text.len(), || {
                checks += 1;
                checks == 1
            }),
            Err(ClipboardReadError::Denied)
        );
        assert_eq!(checks, 2);
        assert_eq!(
            read_board(&private_board(&[0xff]), 1012, || true),
            Err(ClipboardReadError::Unavailable)
        );
        assert_eq!(
            read_board(&private_board(&[]), 0, || true),
            Ok(String::new())
        );
    }

    #[test]
    fn off_main_thread_cannot_read_or_request_authorization() {
        let result =
            std::thread::spawn(|| read_clipboard_text(1012, || panic!("off-main authorization")))
                .join();
        assert_eq!(result.ok(), Some(Err(ClipboardReadError::Unavailable)));
    }
}
