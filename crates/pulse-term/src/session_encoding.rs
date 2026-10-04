//! Bounded normalization owned by the terminal actor. GBK is validation text-only.
use super::{MAX_CHUNK, SessionError};
#[derive(Clone, Copy)]
pub(super) enum Mode {
    Utf8,
    #[cfg(feature = "validation")]
    Gbk,
}
impl Mode {
    pub fn source_is_utf8(self) -> bool {
        matches!(self, Self::Utf8)
    }
    pub fn encode(self, bytes: Vec<u8>) -> Result<Vec<u8>, SessionError> {
        match self {
            Self::Utf8 => Ok(bytes),
            #[cfg(feature = "validation")]
            Self::Gbk => {
                let text = std::str::from_utf8(&bytes).map_err(|_| SessionError::Encoding)?;
                let (encoded, _, lossy) = encoding_rs::GBK.encode(text);
                if lossy || encoded.len() > MAX_CHUNK {
                    Err(SessionError::Encoding)
                } else {
                    Ok(encoded.into_owned())
                }
            }
        }
    }
}
pub(super) struct Decoder {
    mode: Mode,
    #[cfg(feature = "validation")]
    gbk: Option<encoding_rs::Decoder>,
    pending: Vec<u8>,
    at: usize,
}
impl Decoder {
    pub fn new(mode: Mode) -> Self {
        Self {
            mode,
            #[cfg(feature = "validation")]
            gbk: if mode.source_is_utf8() {
                None
            } else {
                Some(encoding_rs::GBK.new_decoder_without_bom_handling())
            },
            pending: vec![],
            at: 0,
        }
    }
    pub async fn next(
        &mut self,
        raw: &mut tokio::sync::mpsc::Receiver<Vec<u8>>,
    ) -> Option<Result<(Vec<u8>, u64), SessionError>> {
        if self.at < self.pending.len() {
            let end = (self.at + MAX_CHUNK).min(self.pending.len());
            let bytes = self.pending[self.at..end].to_vec();
            self.at = end;
            if self.at == self.pending.len() {
                self.pending = vec![];
                self.at = 0;
            }
            return Some(Ok((bytes, 0)));
        }
        let bytes = raw.recv().await?;
        if bytes.len() > MAX_CHUNK {
            return Some(Err(SessionError::ChunkTooLarge));
        }
        let source = bytes.len() as u64;
        if self.mode.source_is_utf8() {
            return Some(Ok((bytes, source)));
        }
        #[cfg(feature = "validation")]
        {
            let decoder = self.gbk.as_mut()?;
            let mut output = String::with_capacity(3 * MAX_CHUNK + 4);
            let (result, consumed) =
                decoder.decode_to_string_without_replacement(&bytes, &mut output, false);
            if result != encoding_rs::DecoderResult::InputEmpty || consumed != bytes.len() {
                return Some(Err(SessionError::Encoding));
            }
            self.pending = output.into_bytes();
            let end = self.pending.len().min(MAX_CHUNK);
            let first = self.pending[..end].to_vec();
            self.at = end;
            if self.at == self.pending.len() {
                self.pending = vec![];
                self.at = 0;
            }
            Some(Ok((first, source)))
        }
        #[cfg(not(feature = "validation"))]
        {
            Some(Err(SessionError::Encoding))
        }
    }
    pub fn finish(&mut self) -> bool {
        if self.at < self.pending.len() {
            return false;
        }
        #[cfg(feature = "validation")]
        if let Some(decoder) = &mut self.gbk {
            let mut output = String::with_capacity(4);
            let (result, consumed) =
                decoder.decode_to_string_without_replacement(&[], &mut output, true);
            return result == encoding_rs::DecoderResult::InputEmpty
                && consumed == 0
                && output.is_empty();
        }
        true
    }
}
