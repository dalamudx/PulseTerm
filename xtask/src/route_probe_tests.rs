#![allow(clippy::unwrap_used)]
use super::*;
fn report() -> Value {
    let cases=["direct","http-socks","socks-http","http-denied","socks-denied"].map(|name|{let denied=name.ends_with("denied");let c=match name{"direct"=>[[1,0],[0,0],[0,0]],"http-denied"=>[[0,0],[0,0],[1,0]],"socks-denied"=>[[0,0],[1,0],[1,1]],_=>[[1,0],[1,1],[1,1]]};json!({"mode":name,"authenticatedProxyRefusal":denied,"binaryBytesExact":!denied,"firstBinaryPrefixPreserved":!denied,"halfCloseAndFinalEof":!denied,"nextHopAdmissionExact":true,"allListenersAndRelaysJoined":true,"payloadBytes":if denied{0}else{PAYLOAD},"prefixBytes":if denied{0}else{PREFIX.len()},"targetCounts":c[0],"innerCounts":c[1],"outerCounts":c[2]})});
    json!({"schemaVersion":1,"completed":true,"cases":cases,"limits":["loopback fixture"]})
}
#[test]
fn route_report_requires_exact_payload_both_orders_and_real_endpoint_counts() {
    assert!(validate(&report()).is_ok());
    assert!(validate(&json!({"completed":true})).is_err());
    let mut r = report();
    r["cases"][1]["payloadBytes"] = json!(0);
    assert!(validate(&r).is_err());
    r = report();
    r["cases"][2]["mode"] = json!("http-socks");
    assert!(validate(&r).is_err());
    r = report();
    r["cases"][3]["targetCounts"] = json!([1, 0]);
    assert!(validate(&r).is_err());
}
#[test]
fn route_eof_prefix_and_cleanup_cannot_be_inferred_from_authentication() {
    for field in [
        "binaryBytesExact",
        "firstBinaryPrefixPreserved",
        "halfCloseAndFinalEof",
        "allListenersAndRelaysJoined",
    ] {
        let mut r = report();
        r["cases"][1][field] = json!(false);
        assert!(validate(&r).is_err());
    }
}
