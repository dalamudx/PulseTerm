//! Bounded Agent framing and private one-use russh signer.
use super::{AgentError, AgentPublicKey, Authorize};
use russh::{
    Signer,
    keys::{
        PublicKey,
        agent::AgentIdentity,
        signature::Verifier,
        ssh_key::{HashAlg, Signature},
    },
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
const MAX_PACKET: usize = 256 * 1024;
const MAX_KEYS: u32 = 128;
const MAX_FIELD: usize = 16 * 1024;
pub(super) struct Wire {
    stream: tokio::net::UnixStream,
}
impl Wire {
    pub(super) fn new(stream: tokio::net::UnixStream) -> Self {
        Self { stream }
    }
    pub(super) async fn packet(
        &mut self,
        body: &[u8],
        current: &Authorize,
    ) -> Result<Vec<u8>, AgentError> {
        if !current() {
            return Err(AgentError::Denied);
        }
        if body.is_empty() || body.len() > MAX_PACKET {
            return Err(AgentError::Protocol);
        }
        let size = u32::try_from(body.len()).map_err(|_| AgentError::Protocol)?;
        let result = tokio::time::timeout(std::time::Duration::from_secs(5), async {
            self.stream
                .write_all(&size.to_be_bytes())
                .await
                .map_err(|_| AgentError::Unavailable)?;
            self.stream
                .write_all(body)
                .await
                .map_err(|_| AgentError::Unavailable)?;
            self.stream
                .flush()
                .await
                .map_err(|_| AgentError::Unavailable)?;
            let length = self
                .stream
                .read_u32()
                .await
                .map_err(|_| AgentError::Unavailable)?;
            let length = usize::try_from(length).map_err(|_| AgentError::Protocol)?;
            if length == 0 || length > MAX_PACKET {
                return Err(AgentError::Protocol);
            }
            if !current() {
                return Err(AgentError::Denied);
            }
            let mut response = vec![0; length];
            self.stream
                .read_exact(&mut response)
                .await
                .map_err(|_| AgentError::Unavailable)?;
            Ok(response)
        })
        .await
        .map_err(|_| AgentError::Unavailable)??;
        if !current() {
            return Err(AgentError::Denied);
        }
        Ok(result)
    }
}
struct Cursor<'a> {
    remaining: &'a [u8],
}
impl<'a> Cursor<'a> {
    fn take(&mut self, count: usize) -> Result<&'a [u8], AgentError> {
        let result = self.remaining.get(..count).ok_or(AgentError::Protocol)?;
        self.remaining = self.remaining.get(count..).ok_or(AgentError::Protocol)?;
        Ok(result)
    }
    fn byte(&mut self) -> Result<u8, AgentError> {
        self.take(1)?.first().copied().ok_or(AgentError::Protocol)
    }
    fn number(&mut self) -> Result<u32, AgentError> {
        let bytes: [u8; 4] = self.take(4)?.try_into().map_err(|_| AgentError::Protocol)?;
        Ok(u32::from_be_bytes(bytes))
    }
    fn field(&mut self) -> Result<&'a [u8], AgentError> {
        let length = usize::try_from(self.number()?).map_err(|_| AgentError::Protocol)?;
        if length > MAX_FIELD {
            return Err(AgentError::Protocol);
        }
        self.take(length)
    }
    fn finish(self) -> Result<(), AgentError> {
        if self.remaining.is_empty() {
            Ok(())
        } else {
            Err(AgentError::Protocol)
        }
    }
}
pub(super) fn decode_keys(response: &[u8]) -> Result<Vec<AgentPublicKey>, AgentError> {
    let mut reader = Cursor {
        remaining: response,
    };
    if reader.byte()? != 12 {
        return Err(AgentError::Unavailable);
    }
    let count = reader.number()?;
    if count > MAX_KEYS {
        return Err(AgentError::Protocol);
    }
    let mut result: Vec<AgentPublicKey> = Vec::new();
    for _ in 0..count {
        let raw = reader.field()?;
        let _comment = reader.field()?;
        let Ok(key) = PublicKey::from_bytes(raw) else {
            continue;
        };
        let algorithm = key.algorithm().to_string();
        let fingerprint = key.fingerprint(HashAlg::Sha256).to_string();
        if super::SshAgentIdentity::new("Agent key", "user", &algorithm, &fingerprint).is_err() {
            continue;
        }
        if result
            .iter()
            .any(|previous| previous.algorithm == algorithm && previous.fingerprint == fingerprint)
        {
            return Err(AgentError::Protocol);
        }
        result.push(AgentPublicKey {
            key,
            algorithm,
            fingerprint,
        });
    }
    reader.finish()?;
    Ok(result)
}
fn field(output: &mut Vec<u8>, data: &[u8]) -> Result<(), AgentError> {
    if data.len() > MAX_FIELD {
        return Err(AgentError::Protocol);
    }
    let length = u32::try_from(data.len()).map_err(|_| AgentError::Protocol)?;
    output.extend_from_slice(&length.to_be_bytes());
    output.extend_from_slice(data);
    Ok(())
}
pub(super) struct SelectedSigner {
    pub(super) wire: Wire,
    pub(super) key: PublicKey,
    pub(super) current: Authorize,
    pub(super) used: bool,
}
impl Signer for SelectedSigner {
    type Error = AgentError;
    async fn auth_sign(
        &mut self,
        key: &AgentIdentity,
        hash: Option<HashAlg>,
        mut data: Vec<u8>,
    ) -> Result<Vec<u8>, Self::Error> {
        if self.used || !(self.current)() {
            return Err(AgentError::Denied);
        }
        match key {
            AgentIdentity::PublicKey { key, .. } if key == &self.key => {}
            _ => return Err(AgentError::Denied),
        }
        let flags: u32 = if self.key.algorithm().is_rsa() {
            match hash {
                Some(HashAlg::Sha256) => 2,
                Some(HashAlg::Sha512) => 4,
                _ => return Err(AgentError::Denied),
            }
        } else {
            if hash.is_some() {
                return Err(AgentError::Denied);
            }
            0
        };
        self.used = true;
        let mut request = vec![13];
        field(
            &mut request,
            &self.key.to_bytes().map_err(|_| AgentError::Protocol)?,
        )?;
        field(&mut request, &data)?;
        request.extend_from_slice(&flags.to_be_bytes());
        let response = self.wire.packet(&request, &self.current).await?;
        if response.as_slice() == [5] {
            return Err(AgentError::Refused);
        }
        let mut reader = Cursor {
            remaining: &response,
        };
        if reader.byte()? != 14 {
            return Err(AgentError::Protocol);
        }
        let blob = reader.field()?;
        reader.finish()?;
        let signature = Signature::try_from(blob).map_err(|_| AgentError::Protocol)?;
        if Vec::<u8>::try_from(signature.clone())
            .map_err(|_| AgentError::Protocol)?
            .as_slice()
            != blob
        {
            return Err(AgentError::Protocol);
        }
        let expected = match flags {
            2 => "rsa-sha2-256".to_owned(),
            4 => "rsa-sha2-512".to_owned(),
            _ => self.key.algorithm().to_string(),
        };
        if signature.algorithm().to_string() != expected
            || Verifier::verify(&self.key, &data, &signature).is_err()
        {
            return Err(AgentError::Protocol);
        }
        if !(self.current)() {
            return Err(AgentError::Denied);
        }
        field(&mut data, blob)?;
        Ok(data)
    }
}
