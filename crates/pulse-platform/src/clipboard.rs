//! Fixed native clipboard failures; no clipboard bytes, paths or native diagnostics.

/// A restricted plain-text read failed without exposing clipboard contents.
#[derive(Clone, Copy, Debug, Eq, PartialEq, thiserror::Error)]
pub enum ClipboardReadError {
    /// The original caller is no longer authorized.
    #[error("clipboard.denied")]
    Denied,
    /// Native plain text or its required encoding is unavailable.
    #[error("clipboard.unavailable")]
    Unavailable,
    /// The native text exceeds the caller's byte budget.
    #[error("clipboard.too_large")]
    TooLarge,
}
