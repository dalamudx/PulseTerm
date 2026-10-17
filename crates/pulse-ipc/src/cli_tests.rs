#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::json;
const SECRET: [u8; 64] = [b'a'; 64];
fn request(id: &str, method: &str, params: Value) -> Request {
    Request::parse(
        &serde_json::to_vec(&json!({"jsonrpc":"2.0","id":id,"method":method,"params":params}))
            .unwrap(),
    )
    .unwrap()
}
fn hello(id: &str, token: &str) -> Request {
    request(id, "hello", json!({"protocolVersion":1,"token":token}))
}
fn gate() -> SessionAuth {
    SessionAuth::new(Zeroizing::new(SECRET))
}
#[test]
fn duplicate_cli_parameter_keys_are_rejected_before_auth_or_dispatch() {
    for body in [
        br#"{"jsonrpc":"2.0","id":"one","method":"hello","params":{"protocolVersion":2,"protocolVersion":1}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":{"profile":"first","profile":"second"}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":{"target":{"window":"first","window":"second"}}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":{"target":[{"pane":"first","pane":"second"}]}}"#.as_slice(),
    ] {
        assert!(matches!(Request::parse(body), Err(CliError::Request)));
    }
}

#[test]
fn unique_cli_params_preserve_all_valid_json_scalar_and_nested_values()
-> Result<(), Box<dyn std::error::Error>> {
    let body = r#"{"jsonrpc":"2.0","id":"values","method":"probe","params":{"text":"中文🌍\\\"","yes":true,"no":false,"nil":null,"negative":-9223372036854775808,"maximum":18446744073709551615,"float":1.25,"negativeZero":-0.0,"array":[null,true,-5,1.25,{"same":"a"},{"same":"b"}],"object":{"same":"c"}}}"#;
    let parsed = Request::parse(body.as_bytes())?;
    let ordinary: Value = serde_json::from_str(body)?;
    assert_eq!(Value::Object(parsed.params().clone()), ordinary["params"]);
    let omitted = Request::parse(br#"{"jsonrpc":"2.0","id":"empty","method":"probe"}"#)?;
    assert!(omitted.params().is_empty());
    Ok(())
}
#[test]
fn escaped_duplicate_parameter_names_and_nonobjects_are_rejected() {
    for body in [
        br#"{"jsonrpc":"2.0","id":"one","method":"hello","params":{"token":"one","\u0074oken":"two"}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":{"target":{"window":"one","\u0077indow":"two"}}}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":null}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":[]}"#.as_slice(),
        br#"{"jsonrpc":"2.0","id":"one","method":"open","params":{"number":1e999}}"#.as_slice(),
    ] { assert!(matches!(Request::parse(body), Err(CliError::Request))); }
}
#[test]
fn unique_parameter_parser_keeps_json_recursion_and_wire_budget_limits() {
    let prefix = r#"{"jsonrpc":"2.0","id":"deep","method":"probe","params":{"data":"#;
    let valid = format!("{prefix}{}null{}{}}}", "[".repeat(8), "]".repeat(8), "}");
    assert!(Request::parse(valid.as_bytes()).is_ok());
    let too_deep = format!(
        "{prefix}{}null{}{}}}",
        "[".repeat(130),
        "]".repeat(130),
        "}"
    );
    assert!(matches!(
        Request::parse(too_deep.as_bytes()),
        Err(CliError::Request)
    ));
    assert!(matches!(
        Request::parse(&vec![b' '; MAX_BODY + 1]),
        Err(CliError::Length)
    ));
}

#[test]
fn one_frame_per_push_even_with_coalesced_frames() {
    let first = encode(&json!({"id":"one","text":"中文🌍"})).unwrap();
    let second = encode(&json!({"id":"two"})).unwrap();
    let mut bytes = first.to_vec();
    bytes.extend_from_slice(&second);
    let mut d = Decoder::default();
    let (n, a) = d.push(&bytes).unwrap();
    assert_eq!(n, first.len());
    assert_eq!(
        serde_json::from_slice::<Value>(&a.unwrap()).unwrap()["id"],
        "one"
    );
    let (m, b) = d.push(&bytes[n..]).unwrap();
    assert_eq!(m, second.len());
    assert_eq!(
        serde_json::from_slice::<Value>(&b.unwrap()).unwrap()["id"],
        "two"
    );
    d.finish().unwrap();
    assert!(matches!(d.push(&[]), Err(CliError::Closed)));
}
#[test]
fn all_splits_keep_header_and_multibyte_json_exact() {
    let wire = encode(&json!({"text":"中文🌍"})).unwrap();
    for split in 0..=wire.len() {
        let mut d = Decoder::default();
        let (n, first) = d.push(&wire[..split]).unwrap();
        assert_eq!(n, split);
        let body = if let Some(first) = first {
            first
        } else {
            let (_, last) = d.push(&wire[split..]).unwrap();
            last.unwrap()
        };
        assert_eq!(
            serde_json::from_slice::<Value>(&body).unwrap()["text"],
            "中文🌍"
        );
        d.finish().unwrap();
    }
}
#[test]
fn declared_invalid_length_is_rejected_before_body_allocation() {
    for n in [0, MAX_BODY as u32 + 1, u32::MAX] {
        let mut d = Decoder::default();
        assert!(matches!(d.push(&n.to_le_bytes()), Err(CliError::Length)));
        assert_eq!(d.body.capacity(), 0);
        assert!(matches!(d.push(b"more"), Err(CliError::Closed)));
    }
}
#[test]
fn eof_inside_any_nonfinal_byte_is_truncated() {
    let wire = encode(&json!({"x":1})).unwrap();
    for n in 1..wire.len() {
        let mut d = Decoder::default();
        d.push(&wire[..n]).unwrap();
        assert_eq!(d.finish(), Err(CliError::Truncated));
        assert!(d.body.is_empty());
    }
    assert_eq!(Decoder::default().finish(), Ok(()));
}
#[test]
fn encoder_limit_stops_serialization_and_decoder_can_accept_exact_max() {
    assert_eq!(
        encode(&Value::String("x".repeat(MAX_BODY))).err(),
        Some(CliError::Length)
    );
    let mut wire = (MAX_BODY as u32).to_le_bytes().to_vec();
    wire.extend(vec![b'x'; MAX_BODY]);
    let mut d = Decoder::default();
    let (n, b) = d.push(&wire).unwrap();
    assert_eq!(n, wire.len());
    assert_eq!(b.unwrap().len(), MAX_BODY);
}
#[test]
fn malformed_jsonrpc_subset_is_not_a_request() {
    for s in [
        r#"[]"#,
        r#"{"jsonrpc":"1.0","id":"1","method":"probe.ping"}"#,
        r#"{"jsonrpc":"2.0","method":"probe.ping"}"#,
        r#"{"jsonrpc":"2.0","id":1,"method":"probe.ping"}"#,
        r#"{"jsonrpc":"2.0","id":"x","method":"probe.ping","params":[]}"#,
        r#"{"jsonrpc":"2.0","id":"x","id":"y","method":"probe.ping"}"#,
        r#"{"jsonrpc":"2.0","id":"x","method":"probe.ping","extra":1}"#,
    ] {
        assert!(Request::parse(s.as_bytes()).is_err());
    }
    assert!(Request::parse(&[0xff]).is_err());
    assert!(Request::parse(&vec![b' '; MAX_BODY + 1]).is_err());
}
#[test]
fn no_dispatch_before_hello_and_failure_is_terminal() {
    let mut g = gate();
    assert!(matches!(
        g.admit(request("1", "probe.ping", json!({}))),
        Err(CliError::Handshake)
    ));
    assert!(g.expected.is_none());
    assert!(matches!(
        g.admit(hello("2", &"a".repeat(64))),
        Err(CliError::Closed)
    ));
}
#[test]
fn only_matching_secret_and_version_unlocks_gate() {
    for params in [
        json!({"protocolVersion":1,"token":"b".repeat(64)}),
        json!({"protocolVersion":2,"token":"a".repeat(64)}),
        json!({"protocolVersion":1,"token":"a".repeat(63)}),
        json!({"protocolVersion":1,"token":"a".repeat(64),"extra":1}),
    ] {
        let mut g = gate();
        assert!(matches!(
            g.admit(request("1", "hello", params)),
            Err(CliError::Authentication)
        ));
        assert!(g.expected.is_none());
    }
    let mut g = gate();
    assert!(matches!(
        g.admit(hello("1", &"a".repeat(64))),
        Ok(Admission::Hello { .. })
    ));
    assert!(g.expected.is_none());
    assert!(matches!(
        g.admit(request("2", "probe.ping", json!({"text":"中文"}))),
        Ok(Admission::Call(_))
    ));
}
#[test]
fn hello_replay_duplicate_ids_and_budget_close_connection() {
    let mut g = gate();
    g.admit(hello("1", &"a".repeat(64))).unwrap();
    assert!(matches!(
        g.admit(hello("2", &"a".repeat(64))),
        Err(CliError::Handshake)
    ));
    let mut g = gate();
    g.admit(hello("1", &"a".repeat(64))).unwrap();
    assert!(matches!(
        g.admit(request("1", "probe.ping", json!({}))),
        Err(CliError::Budget)
    ));
    let mut g = gate();
    g.admit(hello("0", &"a".repeat(64))).unwrap();
    for n in 1..MAX_REQUESTS {
        g.admit(request(&n.to_string(), "probe.ping", json!({})))
            .unwrap();
    }
    assert!(matches!(
        g.admit(request("overflow", "probe.ping", json!({}))),
        Err(CliError::Budget)
    ));
}
proptest::proptest! {#[test]fn arbitrary_body_chunking_is_lossless(chunk in 1usize..65){let wire=encode(&json!({"text":"中文🌍".repeat(32)})).unwrap();let mut d=Decoder::default();let mut got=None;for block in wire.chunks(chunk){let(n,b)=d.push(block).unwrap();proptest::prop_assert_eq!(n,block.len());if b.is_some(){proptest::prop_assert!(got.is_none());got=b;}}let actual=serde_json::from_slice::<Value>(&got.unwrap()).unwrap();let expected="中文🌍".repeat(32);proptest::prop_assert_eq!(actual["text"].as_str(),Some(expected.as_str()));d.finish().unwrap();}}
