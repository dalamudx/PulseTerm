//! Protocol-independent direct/HTTP CONNECT/SOCKS5 byte routes.
//! Trusted backend configuration only; no frontend/CLI entry or implicit SSH reuse.
#![forbid(unsafe_code)]
#[cfg(test)]
mod security_tests;
#[cfg(test)]
mod tests;
use base64::Engine as _;
use std::{
    future::Future,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt},
    net::TcpStream,
};
use zeroize::Zeroizing;
/// Bound route depth before allocating sockets or doing DNS.
pub const MAX_HOPS: usize = 8;
/// Bound response headers before UTF-8 parsing. Never consume tunneled application bytes.
pub const MAX_HTTP_HEADER: usize = 8192;
/// Safe static route failures: no secret/header/request text in error messages.
#[derive(Debug, thiserror::Error)]
pub enum RouteError {
    /// Invalid host, port, credential or deadline.
    #[error("route.invalid_config")]
    Config,
    /// Proxy authentication was denied or unexpectedly downgraded.
    #[error("route.auth_failed")]
    Authentication,
    /// Valid proxy refusal to connect to the target.
    #[error("route.proxy_refused")]
    Refused,
    /// Malformed or unsupported handshake.
    #[error("route.proxy_protocol")]
    Protocol,
    /// Proxy response exceeds the header budget.
    #[error("route.proxy_header_limit")]
    HeaderLimit,
    /// Total dial/chain negotiation deadline expired.
    #[error("route.timeout")]
    Timeout,
    /// Transport I/O error (not remote protocol contents).
    #[error("route.io")]
    Io(#[from] std::io::Error),
}
/// Normalized destination, kept private to prevent header injection through unchecked fields.
#[derive(Clone)]
pub struct Destination {
    host: String,
    port: u16,
}
impl Destination {
    /// ASCII DNS or an IP literal; internationalized names must be normalized to IDNA upstream.
    pub fn new(host: String, port: u16) -> Result<Self, RouteError> {
        if port == 0 || host.is_empty() || host.len() > 253 {
            return Err(RouteError::Config);
        }
        if host.parse::<IpAddr>().is_err() {
            let name = host.strip_suffix('.').unwrap_or(&host);
            if name.split('.').any(|s| {
                s.is_empty()
                    || s.len() > 63
                    || s.starts_with('-')
                    || s.ends_with('-')
                    || !s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
            }) {
                return Err(RouteError::Config);
            }
        }
        Ok(Self { host, port })
    }
    fn from_socket(address: SocketAddr) -> Result<Self, RouteError> {
        Self::new(address.ip().to_string(), address.port())
    }
    fn authority(&self) -> String {
        if matches!(self.host.parse::<IpAddr>(), Ok(IpAddr::V6(_))) {
            format!("[{}]:{}", self.host, self.port)
        } else {
            format!("{}:{}", self.host, self.port)
        }
    }
}
/// Explicit username/password, zeroized on drop and intentionally not Debug/Display.
pub struct Credentials {
    username: Zeroizing<String>,
    password: Zeroizing<String>,
}
impl Credentials {
    /// Bound credentials for SOCKS RFC1929 and forbid ambiguous Basic username separators.
    pub fn new(username: String, password: String) -> Result<Self, RouteError> {
        let username = Zeroizing::new(username);
        let password = Zeroizing::new(password);
        if username.is_empty()
            || username.len() > 255
            || password.is_empty()
            || password.len() > 255
            || username.contains(':')
            || username.chars().any(char::is_control)
        {
            return Err(RouteError::Config);
        }
        Ok(Self { username, password })
    }
}
/// A validated proxy hop. Endpoint/credentials come only from a trusted backend profile.
pub struct Hop {
    address: SocketAddr,
    kind: Kind,
    credentials: Option<Credentials>,
}
enum Kind {
    Http,
    Socks,
}
impl Hop {
    /// Negotiate HTTP/1 CONNECT (TLS to an HTTPS proxy is not yet provided).
    pub fn http_connect(
        address: SocketAddr,
        credentials: Option<Credentials>,
    ) -> Result<Self, RouteError> {
        Self::new(address, Kind::Http, credentials)
    }
    /// Negotiate SOCKS5 CONNECT, with explicit no-auth or username/password (no downgrade).
    pub fn socks5(
        address: SocketAddr,
        credentials: Option<Credentials>,
    ) -> Result<Self, RouteError> {
        Self::new(address, Kind::Socks, credentials)
    }
    fn new(
        address: SocketAddr,
        kind: Kind,
        credentials: Option<Credentials>,
    ) -> Result<Self, RouteError> {
        if address.port() == 0 || address.ip().is_unspecified() {
            return Err(RouteError::Config);
        }
        Ok(Self {
            address,
            kind,
            credentials,
        })
    }
}
/// Bounded chain; empty hops means a direct byte stream. No protocol-specific RDP/VNC logic.
pub struct Route {
    hops: Vec<Hop>,
}
impl Route {
    /// Configure a chain without initiating I/O; cancellation of dial drops the partial stream.
    pub fn new(hops: Vec<Hop>) -> Result<Self, RouteError> {
        if hops.len() > MAX_HOPS {
            return Err(RouteError::Config);
        }
        Ok(Self { hops })
    }
}
/// A transport-neutral duplex stream usable by protocol clients.
pub trait Duplex: AsyncRead + AsyncWrite + Unpin + Send {}
impl<T: AsyncRead + AsyncWrite + Unpin + Send> Duplex for T {}
/// Erased stream, leaving protocol engines independent of proxy hop implementations.
pub type BoxStream = Box<dyn Duplex>;
/// Typed dial result with a Send future (no macro needed).
pub type DialFuture<'a> = Pin<Box<dyn Future<Output = Result<BoxStream, RouteError>> + Send + 'a>>;
/// Shared entry point for direct/proxy routing; SSH/ProxyCommand remain separately gated.
pub trait Dialer {
    /// Open and negotiate the entire route under one total deadline, not sliding per-byte timers.
    fn dial<'a>(&'a self, route: &'a Route, target: &'a Destination) -> DialFuture<'a>;
}
/// Direct/HTTP/SOCKS implementation. No filesystem, credential discovery or execution.
pub struct NetworkDialer {
    deadline: Duration,
}
impl NetworkDialer {
    /// Set a nonzero total deadline of at most thirty seconds.
    pub fn new(deadline: Duration) -> Result<Self, RouteError> {
        if deadline.is_zero() || deadline > Duration::from_secs(30) {
            return Err(RouteError::Config);
        }
        Ok(Self { deadline })
    }
}
impl Dialer for NetworkDialer {
    fn dial<'a>(&'a self, route: &'a Route, target: &'a Destination) -> DialFuture<'a> {
        Box::pin(async move {
            tokio::time::timeout(self.deadline, async {
                let mut stream = if let Some(first) = route.hops.first() {
                    TcpStream::connect(first.address).await?
                } else {
                    TcpStream::connect((target.host.as_str(), target.port)).await?
                };
                for (index, hop) in route.hops.iter().enumerate() {
                    let next = if let Some(next) = route.hops.get(index + 1) {
                        Destination::from_socket(next.address)?
                    } else {
                        target.clone()
                    };
                    match hop.kind {
                        Kind::Http => http(&mut stream, &next, hop.credentials.as_ref()).await?,
                        Kind::Socks => socks(&mut stream, &next, hop.credentials.as_ref()).await?,
                    }
                }
                Ok::<BoxStream, RouteError>(Box::new(stream))
            })
            .await
            .map_err(|_| RouteError::Timeout)?
        })
    }
}
async fn http<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    target: &Destination,
    credentials: Option<&Credentials>,
) -> Result<(), RouteError> {
    let authority = target.authority();
    let mut header = Zeroizing::new(format!(
        "CONNECT {authority} HTTP/1.1\r\nHost: {authority}\r\n"
    ));
    if let Some(c) = credentials {
        let clear = Zeroizing::new(format!("{}:{}", c.username.as_str(), c.password.as_str()));
        let encoded =
            Zeroizing::new(base64::engine::general_purpose::STANDARD.encode(clear.as_bytes()));
        header.push_str("Proxy-Authorization: Basic ");
        header.push_str(encoded.as_str());
        header.push_str("\r\n");
    }
    header.push_str("\r\n");
    stream.write_all(header.as_bytes()).await?;
    let mut response = Vec::with_capacity(256);
    loop {
        if response.len() == MAX_HTTP_HEADER {
            return Err(RouteError::HeaderLimit);
        }
        let mut byte = [0u8; 1];
        stream.read_exact(&mut byte).await?;
        response.push(byte[0]);
        if response.ends_with(b"\r\n\r\n") {
            break;
        }
    }
    parse_http_response(&response)
}
fn parse_http_response(response: &[u8]) -> Result<(), RouteError> {
    let end = response
        .windows(2)
        .position(|b| b == b"\r\n")
        .ok_or(RouteError::Protocol)?;
    let line = &response[..end];
    if line.len() < 12
        || !matches!(&line[..8], b"HTTP/1.0" | b"HTTP/1.1")
        || line[8] != b' '
        || !line[9..12].iter().all(u8::is_ascii_digit)
        || (line.len() > 12 && line[12] != b' ')
        || line.iter().any(|b| *b < 32 || *b > 126)
    {
        return Err(RouteError::Protocol);
    }
    let mut remaining = &response[end + 2..];
    while remaining != b"\r\n" {
        let end = remaining
            .windows(2)
            .position(|b| b == b"\r\n")
            .ok_or(RouteError::Protocol)?;
        let header = &remaining[..end];
        let colon = header
            .iter()
            .position(|b| *b == b':')
            .ok_or(RouteError::Protocol)?;
        if colon == 0
            || !header[..colon]
                .iter()
                .all(|b| b.is_ascii_alphanumeric() || b"!#$%&'*+-.^_`|~".contains(b))
            || header[colon + 1..]
                .iter()
                .any(|b| (*b < 32 && *b != b'\t') || *b == 127)
        {
            return Err(RouteError::Protocol);
        }
        remaining = &remaining[end + 2..];
    }
    let code = u16::from(line[9] - b'0') * 100
        + u16::from(line[10] - b'0') * 10
        + u16::from(line[11] - b'0');
    match code {
        200..=299 => Ok(()),
        407 => Err(RouteError::Authentication),
        100..=599 => Err(RouteError::Refused),
        _ => Err(RouteError::Protocol),
    }
}
async fn socks<S: AsyncRead + AsyncWrite + Unpin>(
    stream: &mut S,
    target: &Destination,
    credentials: Option<&Credentials>,
) -> Result<(), RouteError> {
    let method = if credentials.is_some() { 2 } else { 0 };
    stream.write_all(&[5, 1, method]).await?;
    let mut choice = [0u8; 2];
    stream.read_exact(&mut choice).await?;
    if choice[0] != 5 {
        return Err(RouteError::Protocol);
    }
    if choice[1] != method {
        return Err(RouteError::Authentication);
    }
    if let Some(c) = credentials {
        let mut auth = Zeroizing::new(Vec::with_capacity(3 + c.username.len() + c.password.len()));
        auth.extend_from_slice(&[1, c.username.len() as u8]);
        auth.extend_from_slice(c.username.as_bytes());
        auth.push(c.password.len() as u8);
        auth.extend_from_slice(c.password.as_bytes());
        stream.write_all(&auth).await?;
        let mut accepted = [0u8; 2];
        stream.read_exact(&mut accepted).await?;
        if accepted[0] != 1 {
            return Err(RouteError::Protocol);
        }
        if accepted[1] != 0 {
            return Err(RouteError::Authentication);
        }
    }
    let mut request = Vec::with_capacity(263);
    request.extend_from_slice(&[5, 1, 0]);
    match target.host.parse::<IpAddr>() {
        Ok(IpAddr::V4(ip)) => {
            request.push(1);
            request.extend_from_slice(&ip.octets());
        }
        Ok(IpAddr::V6(ip)) => {
            request.push(4);
            request.extend_from_slice(&ip.octets());
        }
        Err(_) => {
            request.push(3);
            request.push(target.host.len() as u8);
            request.extend_from_slice(target.host.as_bytes());
        }
    }
    request.extend_from_slice(&target.port.to_be_bytes());
    stream.write_all(&request).await?;
    let mut response = [0u8; 4];
    stream.read_exact(&mut response).await?;
    if response[0] != 5 || response[2] != 0 {
        return Err(RouteError::Protocol);
    }
    if response[1] != 0 {
        return Err(RouteError::Refused);
    }
    let length = match response[3] {
        1 => 4,
        4 => 16,
        3 => {
            let mut length = [0u8; 1];
            stream.read_exact(&mut length).await?;
            if length[0] == 0 {
                return Err(RouteError::Protocol);
            }
            usize::from(length[0])
        }
        _ => return Err(RouteError::Protocol),
    };
    let mut suffix = vec![0u8; length + 2];
    stream.read_exact(&mut suffix).await?;
    Ok(())
}
