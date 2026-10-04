#![allow(clippy::unwrap_used)]
use super::*;
use tokio::io::DuplexStream;
fn target() -> Destination {
    Destination::new("127.0.0.1".into(), 1234).unwrap()
}
async fn header(s: &mut DuplexStream) -> Vec<u8> {
    let mut h = vec![];
    while !h.ends_with(b"\r\n\r\n") {
        let mut b = [0];
        s.read_exact(&mut b).await.unwrap();
        h.push(b[0]);
        assert!(h.len() < MAX_HTTP_HEADER);
    }
    h
}
#[test]
fn configuration_rejects_injection_invalid_ports_credentials_and_depth() {
    for host in [
        "",
        "bad\r\nHost: evil",
        "a/b",
        "bad name",
        "-leading.example",
        "trailing-.example",
        "a..b",
    ] {
        assert!(Destination::new(host.into(), 22).is_err());
    }
    assert!(Destination::new("::1".into(), 22).is_ok());
    assert_eq!(
        Destination::new("::1".into(), 22).unwrap().authority(),
        "[::1]:22"
    );
    assert!(Destination::new("example.com".into(), 0).is_err());
    assert!(Destination::new("x".repeat(254), 22).is_err());
    assert!(Credentials::new("a:b".into(), "secret".into()).is_err());
    assert!(Credentials::new("a".repeat(256), "secret".into()).is_err());
    assert!(NetworkDialer::new(Duration::ZERO).is_err());
    let hops = (0..MAX_HOPS + 1)
        .map(|_| Hop::socks5("127.0.0.1:1".parse().unwrap(), None).unwrap())
        .collect();
    assert!(Route::new(hops).is_err());
}
#[tokio::test]
async fn http_header_and_binary_prefix_in_same_packet_are_lossless() {
    let (mut client, mut server) = tokio::io::duplex(256);
    let remote = tokio::spawn(async move {
        let h = header(&mut server).await;
        assert!(h.starts_with(b"CONNECT 127.0.0.1:1234 HTTP/1.1\r\n"));
        assert!(
            h.windows(b"Proxy-Authorization: Basic dXNlcjpwYXNz".len())
                .any(|s| s == b"Proxy-Authorization: Basic dXNlcjpwYXNz")
        );
        server
            .write_all(b"HTTP/1.1 200 OK\r\n\r\n\0\xffRFB")
            .await
            .unwrap();
    });
    let c = Credentials::new("user".into(), "pass".into()).unwrap();
    http(&mut client, &target(), Some(&c)).await.unwrap();
    let mut bytes = [0u8; 5];
    client.read_exact(&mut bytes).await.unwrap();
    assert_eq!(bytes, *b"\0\xffRFB");
    remote.await.unwrap();
}
#[tokio::test]
async fn http_refusals_bad_status_and_oversized_headers_are_explicit() {
    for (response, kind) in [
        (b"HTTP/1.1 407 Auth\r\n\r\n".to_vec(), 0),
        (b"HTTP/1.1 403 Denied\r\n\r\n".to_vec(), 1),
        (b"not HTTP status\r\n\r\n".to_vec(), 2),
        (vec![b'x'; MAX_HTTP_HEADER + 1], 3),
    ] {
        let (mut client, mut server) = tokio::io::duplex(16384);
        let remote = tokio::spawn(async move {
            header(&mut server).await;
            server.write_all(&response).await.unwrap();
        });
        let result = http(&mut client, &target(), None).await;
        assert!(matches!(
            (kind, result),
            (0, Err(RouteError::Authentication))
                | (1, Err(RouteError::Refused))
                | (2, Err(RouteError::Protocol))
                | (3, Err(RouteError::HeaderLimit))
        ));
        remote.await.unwrap();
    }
}
#[tokio::test]
async fn socks_auth_downgrade_refused_without_sending_credentials() {
    let (mut client, mut server) = tokio::io::duplex(64);
    let remote = tokio::spawn(async move {
        let mut greeting = [0u8; 3];
        server.read_exact(&mut greeting).await.unwrap();
        assert_eq!(greeting, [5, 1, 2]);
        server.write_all(&[5, 0]).await.unwrap();
        let mut tail = vec![];
        server.read_to_end(&mut tail).await.unwrap();
        assert!(tail.is_empty());
    });
    let c = Credentials::new("user".into(), "pass".into()).unwrap();
    assert!(matches!(
        socks(&mut client, &target(), Some(&c)).await,
        Err(RouteError::Authentication)
    ));
    drop(client);
    remote.await.unwrap();
}
#[tokio::test]
async fn socks_ipv4_ipv6_domain_reply_suffix_preserves_application_prefix() {
    for suffix in [
        vec![1, 127, 0, 0, 1, 0, 1],
        {
            let mut v = vec![4];
            v.extend([0; 16]);
            v.extend([0, 1]);
            v
        },
        vec![3, 1, b'x', 0, 1],
    ] {
        let (mut client, mut server) = tokio::io::duplex(128);
        let remote = tokio::spawn(async move {
            let mut g = [0; 3];
            server.read_exact(&mut g).await.unwrap();
            assert_eq!(g, [5, 1, 0]);
            server.write_all(&[5, 0]).await.unwrap();
            let mut request = [0; 10];
            server.read_exact(&mut request).await.unwrap();
            assert_eq!(&request[..4], &[5, 1, 0, 1]);
            let mut response = vec![5, 0, 0];
            response.extend(suffix);
            response.extend(b"\0\xffVNC");
            server.write_all(&response).await.unwrap();
        });
        socks(&mut client, &target(), None).await.unwrap();
        let mut bytes = [0u8; 5];
        client.read_exact(&mut bytes).await.unwrap();
        assert_eq!(bytes, *b"\0\xffVNC");
        remote.await.unwrap();
    }
}
#[tokio::test]
async fn socks_auth_failure_and_bad_reply_do_not_fall_back() {
    for reply in [[1, 1], [2, 0]] {
        let (mut client, mut server) = tokio::io::duplex(128);
        let remote = tokio::spawn(async move {
            let mut g = [0; 3];
            server.read_exact(&mut g).await.unwrap();
            server.write_all(&[5, 2]).await.unwrap();
            let mut prefix = [0; 2];
            server.read_exact(&mut prefix).await.unwrap();
            let mut user = vec![0u8; prefix[1] as usize];
            server.read_exact(&mut user).await.unwrap();
            let mut len = [0];
            server.read_exact(&mut len).await.unwrap();
            let mut pass = vec![0u8; len[0] as usize];
            server.read_exact(&mut pass).await.unwrap();
            assert_eq!(user, b"user");
            assert_eq!(pass, b"pass");
            server.write_all(&reply).await.unwrap();
        });
        let c = Credentials::new("user".into(), "pass".into()).unwrap();
        let r = socks(&mut client, &target(), Some(&c)).await;
        if reply[0] == 1 {
            assert!(matches!(r, Err(RouteError::Authentication)));
        } else {
            assert!(matches!(r, Err(RouteError::Protocol)));
        }
        remote.await.unwrap();
    }
}
#[tokio::test]
async fn total_handshake_timeout_drops_partial_tcp_stream() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let remote = tokio::spawn(async move {
        let (mut s, _) = listener.accept().await.unwrap();
        let mut bytes = vec![];
        s.read_to_end(&mut bytes).await.unwrap();
        assert!(bytes.starts_with(b"CONNECT "));
    });
    let dialer = NetworkDialer::new(Duration::from_millis(100)).unwrap();
    let route = Route::new(vec![Hop::http_connect(addr, None).unwrap()]).unwrap();
    assert!(matches!(
        dialer.dial(&route, &target()).await,
        Err(RouteError::Timeout)
    ));
    tokio::time::timeout(Duration::from_secs(1), remote)
        .await
        .unwrap()
        .unwrap();
}
