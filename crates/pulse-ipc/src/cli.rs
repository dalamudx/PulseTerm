//! Local CLI protocol subset. Transport/peer identity/Capability dispatch are separate.
//! Requests are not executable merely because the application token matched.
use serde::Deserialize;
use serde_json::{Map, Value};
use std::{collections::BTreeSet, io::Write};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;
/// Fixed header: unsigned little-endian JSON body length, unrelated to terminal frames.
pub const HEADER_LEN: usize = 4;
/// Hard maximum per request/response; check before allocating a body.
pub const MAX_BODY: usize = 64 * 1024;
/// Application control protocol version (independent of JSON-RPC's version).
pub const PROTOCOL_VERSION: u16 = 1;
/// Maximum admitted requests per connection, including hello.
pub const MAX_REQUESTS: usize = 128;
/// Owned JSON/wire storage wiped on drop, including transient hello data.
pub type WireBuffer = Zeroizing<Vec<u8>>;
/// Static, content-free protocol errors. No token/params appear in messages.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CliError {
    /// Zero or oversized length.
    #[error("cli.invalid_length")]
    Length,
    /// Incomplete header/body at EOF.
    #[error("cli.truncated")]
    Truncated,
    /// Fatal error or explicit EOF has retired this decoder/connection.
    #[error("cli.closed")]
    Closed,
    /// Unsupported JSON-RPC subset, id, method or params.
    #[error("cli.invalid_request")]
    Request,
    /// Hello was not the first request, or repeated after admission.
    #[error("cli.handshake_required")]
    Handshake,
    /// Invalid token, protocol or hello parameters.
    #[error("cli.authentication_failed")]
    Authentication,
    /// Duplicate request id or total request budget exhausted.
    #[error("cli.request_budget")]
    Budget,
}
/// Incremental bounded framer. One frame maximum per push, never unbounded batch output.
#[derive(Default)]
pub struct Decoder {
    header: [u8; 4],
    header_used: usize,
    needed: Option<usize>,
    body: Zeroizing<Vec<u8>>,
    closed: bool,
}
impl Decoder {
    /// Consume at most one frame and return the consumed input length. Caller owns loop budgets.
    pub fn push(&mut self, input: &[u8]) -> Result<(usize, Option<WireBuffer>), CliError> {
        if self.closed {
            return Err(CliError::Closed);
        }
        let mut used = 0;
        if self.needed.is_none() {
            let n = (4 - self.header_used).min(input.len());
            self.header[self.header_used..self.header_used + n].copy_from_slice(&input[..n]);
            self.header_used += n;
            used += n;
            if self.header_used < 4 {
                return Ok((used, None));
            }
            let needed = u32::from_le_bytes(self.header) as usize;
            if needed == 0 || needed > MAX_BODY {
                self.closed = true;
                return Err(CliError::Length);
            }
            self.needed = Some(needed);
            self.body.reserve_exact(needed);
        }
        if let Some(needed) = self.needed {
            let n = (needed - self.body.len()).min(input.len() - used);
            self.body.extend_from_slice(&input[used..used + n]);
            used += n;
            if self.body.len() == needed {
                let body = std::mem::take(&mut self.body);
                self.header_used = 0;
                self.needed = None;
                return Ok((used, Some(body)));
            }
        }
        Ok((used, None))
    }
    /// EOF retires this framer and refuses pending bytes; never interprets partial JSON.
    pub fn finish(&mut self) -> Result<(), CliError> {
        if self.closed {
            return Err(CliError::Closed);
        }
        self.closed = true;
        if self.header_used != 0 || self.needed.is_some() {
            self.body = Zeroizing::new(vec![]);
            return Err(CliError::Truncated);
        }
        Ok(())
    }
}
struct BodyWriter(Zeroizing<Vec<u8>>);
impl Write for BodyWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if bytes.len() > MAX_BODY - self.0.len() {
            return Err(std::io::Error::other("cli.invalid_length"));
        }
        self.0.extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
/// Serialize JSON with a bounded writer (no oversized intermediate allocation).
/// The returned wire buffer is wiped on drop because hello carries a secret token.
pub fn encode(value: &Value) -> Result<Zeroizing<Vec<u8>>, CliError> {
    let mut writer = BodyWriter(Zeroizing::new(vec![]));
    serde_json::to_writer(&mut writer, value).map_err(|_| CliError::Length)?;
    let mut wire = Zeroizing::new(Vec::with_capacity(HEADER_LEN + writer.0.len()));
    wire.extend_from_slice(&(writer.0.len() as u32).to_le_bytes());
    wire.extend_from_slice(&writer.0);
    Ok(wire)
}
/// JSON-RPC request subset: no batches, notifications, numeric ids or unknown fields.
/// Intentionally has no Debug/Display to keep authentication/parameters out of logs.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    jsonrpc: String,
    id: String,
    method: String,
    #[serde(default, deserialize_with = "unique_params")]
    params: Map<String, Value>,
}
// serde_json::Value's ordinary map decoder silently overwrites duplicate keys.
// Keep JSON's original scalar semantics and recursion limit, but reject ambiguous parameters.
struct UniqueJson(Value);
impl<'de> Deserialize<'de> for UniqueJson {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct Visitor;
        impl<'de> serde::de::Visitor<'de> for Visitor {
            type Value = UniqueJson;
            fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                formatter.write_str("an unambiguous JSON value")
            }
            fn visit_bool<E: serde::de::Error>(self, value: bool) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Bool(value)))
            }
            fn visit_i64<E: serde::de::Error>(self, value: i64) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Number(value.into())))
            }
            fn visit_u64<E: serde::de::Error>(self, value: u64) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Number(value.into())))
            }
            fn visit_f64<E: serde::de::Error>(self, value: f64) -> Result<Self::Value, E> {
                serde_json::Number::from_f64(value)
                    .map(|number| UniqueJson(Value::Number(number)))
                    .ok_or_else(|| E::custom("cli.invalid_request"))
            }
            fn visit_str<E: serde::de::Error>(self, value: &str) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::String(value.into())))
            }
            fn visit_string<E: serde::de::Error>(self, value: String) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::String(value)))
            }
            fn visit_unit<E: serde::de::Error>(self) -> Result<Self::Value, E> {
                Ok(UniqueJson(Value::Null))
            }
            fn visit_seq<A: serde::de::SeqAccess<'de>>(
                self,
                mut sequence: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Vec::new();
                while let Some(value) = sequence.next_element::<UniqueJson>()? {
                    values.push(value.0);
                }
                Ok(UniqueJson(Value::Array(values)))
            }
            fn visit_map<A: serde::de::MapAccess<'de>>(
                self,
                mut map: A,
            ) -> Result<Self::Value, A::Error> {
                let mut values = Map::new();
                while let Some(key) = map.next_key::<String>()? {
                    if values.contains_key(&key) {
                        return Err(serde::de::Error::custom("cli.invalid_request"));
                    }
                    let value = map.next_value::<UniqueJson>()?;
                    values.insert(key, value.0);
                }
                Ok(UniqueJson(Value::Object(values)))
            }
        }
        deserializer.deserialize_any(Visitor)
    }
}
fn unique_params<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Map<String, Value>, D::Error> {
    match UniqueJson::deserialize(deserializer)?.0 {
        Value::Object(map) => Ok(map),
        _ => Err(serde::de::Error::custom("cli.invalid_request")),
    }
}

impl Request {
    /// Parse only a complete bounded UTF-8 JSON object; duplicate fields are rejected, including nested parameter objects.
    pub fn parse(body: &[u8]) -> Result<Self, CliError> {
        if body.is_empty() || body.len() > MAX_BODY {
            return Err(CliError::Length);
        }
        let value: Self = serde_json::from_slice(body).map_err(|_| CliError::Request)?;
        if value.jsonrpc != "2.0" || !identifier(&value.id, 64) || !identifier(&value.method, 96) {
            return Err(CliError::Request);
        }
        Ok(value)
    }
    /// Opaque response correlation id, never a shell command or floating-point number.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Capability name; authentication alone is not permission to dispatch it.
    pub fn method(&self) -> &str {
        &self.method
    }
    /// Object parameters. Consumer must still apply typed Capability validation.
    pub fn params(&self) -> &Map<String, Value> {
        &self.params
    }
}
fn identifier(text: &str, max: usize) -> bool {
    !text.is_empty()
        && text.len() <= max
        && text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_.-".contains(&b))
}
/// One hello acknowledgment or an authenticated request needing Capability authorization.
/// No Debug implementation; hello secrets must not leak via accidental logging.
pub enum Admission {
    /// The secret has been consumed and erased. Reply before handling later requests.
    Hello {
        /// Correlation id of the hello request.
        id: String,
    },
    /// Still requires the shared Capability registry, risk confirmation and target permission.
    Call(Request),
}
/// Per-connection authentication gate, not window/session ownership or a Capability registry.
pub struct SessionAuth {
    expected: Option<Zeroizing<[u8; 64]>>,
    ready: bool,
    closed: bool,
    seen: BTreeSet<String>,
}
impl SessionAuth {
    /// Secret must be minted from OS randomness by the runtime, not remote terminal output.
    /// This gate does not serialize, log, Debug-format or expose the expected secret.
    pub fn new(expected: Zeroizing<[u8; 64]>) -> Self {
        Self {
            expected: Some(expected),
            ready: false,
            closed: false,
            seen: BTreeSet::new(),
        }
    }
    /// Any rejected admission retires this connection and wipes its copy of the secret.
    /// No retries/replays and no request dispatch before successful hello.
    pub fn admit(&mut self, request: Request) -> Result<Admission, CliError> {
        if self.closed {
            return Err(CliError::Closed);
        }
        let result = self.admit_inner(request);
        if result.is_err() {
            self.closed = true;
            self.expected = None;
        }
        result
    }
    fn admit_inner(&mut self, mut request: Request) -> Result<Admission, CliError> {
        if self.seen.len() >= MAX_REQUESTS || !self.seen.insert(request.id.clone()) {
            return Err(CliError::Budget);
        }
        if self.ready {
            if request.method == "hello" {
                return Err(CliError::Handshake);
            }
            return Ok(Admission::Call(request));
        }
        if request.method != "hello" {
            return Err(CliError::Handshake);
        }
        let token = match request.params.remove("token") {
            Some(Value::String(token)) => Zeroizing::new(token),
            _ => return Err(CliError::Authentication),
        };
        let version = request
            .params
            .remove("protocolVersion")
            .and_then(|v| v.as_u64());
        if !request.params.is_empty()
            || version != Some(u64::from(PROTOCOL_VERSION))
            || token.len() != 64
            || !token
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        {
            return Err(CliError::Authentication);
        }
        let expected = self.expected.take().ok_or(CliError::Authentication)?;
        if !bool::from(expected.as_slice().ct_eq(token.as_bytes())) {
            return Err(CliError::Authentication);
        }
        self.ready = true;
        Ok(Admission::Hello { id: request.id })
    }
}
#[cfg(test)]
#[path = "cli_tests.rs"]
mod tests;
