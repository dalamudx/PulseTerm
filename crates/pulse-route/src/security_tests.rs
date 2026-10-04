#![allow(clippy::unwrap_used)]
use super::*;
#[test]
fn connect_status_and_headers_are_strict_but_all_2xx_are_tunnels() {
    for response in [
        b"HTTP/1.1 204 No Content\r\nX-Proxy: value\r\n\r\n".as_slice(),
        b"HTTP/1.0 299\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nX: \xff\t\r\n\r\n",
    ] {
        assert!(parse_http_response(response).is_ok());
    }
    for response in [
        b" HTTP/1.1 200 OK\r\n\r\n".as_slice(),
        b"HTTP/1.1\t200 OK\r\n\r\n",
        b"HTTP/1.1 200OK\r\n\r\n",
        b"HTTP/1.1 200 OK\r\n Invalid: fold\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nInvalid\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nX: \0\r\n\r\n",
        b"HTTP/1.1 000 Invalid\r\n\r\n",
        b"HTTP/1.1 200 OK\r\nX: missing end",
    ] {
        assert!(matches!(
            parse_http_response(response),
            Err(RouteError::Protocol)
        ));
    }
}
#[tokio::test]
async fn dropping_dial_future_closes_the_partially_negotiated_socket() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel();
    let remote = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut header = vec![];
        while !header.ends_with(b"\r\n\r\n") {
            let mut b = [0];
            stream.read_exact(&mut b).await.unwrap();
            header.push(b[0]);
        }
        tx.send(()).unwrap();
        let mut b = [0];
        assert_eq!(stream.read(&mut b).await.unwrap(), 0);
    });
    let dialer = NetworkDialer::new(Duration::from_secs(3)).unwrap();
    let route = Route::new(vec![Hop::http_connect(addr, None).unwrap()]).unwrap();
    let target = Destination::new("127.0.0.1".into(), 1234).unwrap();
    let mut pending = dialer.dial(&route, &target);
    tokio::select! {done=rx=>done.unwrap(),_= &mut pending=>panic!("proxy did not hold handshake")};
    drop(pending);
    tokio::time::timeout(Duration::from_secs(1), remote)
        .await
        .unwrap()
        .unwrap();
}
#[tokio::test]
async fn socks_dns_name_is_forwarded_verbatim_not_locally_resolved() {
    let (mut client, mut server) = tokio::io::duplex(128);
    let remote = tokio::spawn(async move {
        let mut g = [0; 3];
        server.read_exact(&mut g).await.unwrap();
        assert_eq!(g, [5, 1, 0]);
        server.write_all(&[5, 0]).await.unwrap();
        let mut first = [0; 5];
        server.read_exact(&mut first).await.unwrap();
        assert_eq!(&first[..4], &[5, 1, 0, 3]);
        let mut host = vec![0; first[4] as usize];
        server.read_exact(&mut host).await.unwrap();
        assert_eq!(host, b"unresolved.invalid");
        let mut port = [0; 2];
        server.read_exact(&mut port).await.unwrap();
        assert_eq!(u16::from_be_bytes(port), 3389);
        server
            .write_all(&[5, 0, 0, 3, 1, b'x', 0, 1, 0xff])
            .await
            .unwrap();
    });
    let t = Destination::new("unresolved.invalid".into(), 3389).unwrap();
    socks(&mut client, &t, None).await.unwrap();
    assert_eq!(client.read_u8().await.unwrap(), 255);
    remote.await.unwrap();
}
#[tokio::test]
async fn socks_bad_connect_fields_empty_domain_and_refusal_are_explicit() {
    for (response, refused) in [
        (vec![5, 1, 0, 1], true),
        (vec![4, 0, 0, 1], false),
        (vec![5, 0, 1, 1], false),
        (vec![5, 0, 0, 9], false),
        (vec![5, 0, 0, 3, 0], false),
    ] {
        let (mut client, mut server) = tokio::io::duplex(128);
        let remote = tokio::spawn(async move {
            let mut g = [0; 3];
            server.read_exact(&mut g).await.unwrap();
            server.write_all(&[5, 0]).await.unwrap();
            let mut request = [0; 10];
            server.read_exact(&mut request).await.unwrap();
            server.write_all(&response).await.unwrap();
        });
        let t = Destination::new("127.0.0.1".into(), 1).unwrap();
        let r = socks(&mut client, &t, None).await;
        if refused {
            assert!(matches!(r, Err(RouteError::Refused)));
        } else {
            assert!(matches!(r, Err(RouteError::Protocol)));
        }
        remote.await.unwrap();
    }
}
#[tokio::test]
async fn truncated_http_and_socks_are_io_failure_not_accepted_tunnels() {
    let (mut client, mut server) = tokio::io::duplex(256);
    let remote = tokio::spawn(async move {
        let mut header = vec![];
        while !header.ends_with(b"\r\n\r\n") {
            header.push(server.read_u8().await.unwrap());
        }
        server
            .write_all(b"HTTP/1.1 200 OK\r\nX: partial")
            .await
            .unwrap();
    });
    let t = Destination::new("127.0.0.1".into(), 1).unwrap();
    assert!(matches!(
        http(&mut client, &t, None).await,
        Err(RouteError::Io(_))
    ));
    remote.await.unwrap();
    let (mut client, mut server) = tokio::io::duplex(128);
    let remote = tokio::spawn(async move {
        let mut greeting = [0; 3];
        server.read_exact(&mut greeting).await.unwrap();
        server.write_all(&[5]).await.unwrap();
    });
    assert!(matches!(
        socks(&mut client, &t, None).await,
        Err(RouteError::Io(_))
    ));
    remote.await.unwrap();
}
