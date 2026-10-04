#![allow(clippy::unwrap_used)]
use super::*;
fn report() -> Value {
    json!({"sourceBytes":16,"utf8Bytes":19,"pendingCaptureSource":1,"pendingCaptureUtf8":0,"peakCreditBytes":64,"v1Refused":true,"oldLeaseReplaced":true,"boundInputReverseEncoded":true,"backendQueryReplyOnSsh":true,"rawSshEchoExact":true,"binaryV2OffsetsExact":true,"actorScreenExact":true,"sshEofAndClose":true,"orderedEofAcked":true,"limits":["controlled SSH sample"]})
}
#[test]
fn actual_source_display_and_credit_required() {
    let mut r = report();
    assert!(validate(&r).is_ok());
    r["utf8Bytes"] = json!(16);
    assert!(validate(&r).is_err());
    r["utf8Bytes"] = json!(19);
    r["pendingCaptureUtf8"] = json!(1);
    assert!(validate(&r).is_err());
    r["pendingCaptureUtf8"] = json!(0);
    r["peakCreditBytes"] = json!(65);
    assert!(validate(&r).is_err());
    assert!(validate(&json!({"completed":true})).is_err());
}
#[test]
fn no_controlled_network_stage_can_be_omitted() {
    for key in [
        "v1Refused",
        "oldLeaseReplaced",
        "boundInputReverseEncoded",
        "backendQueryReplyOnSsh",
        "rawSshEchoExact",
        "binaryV2OffsetsExact",
        "actorScreenExact",
        "sshEofAndClose",
        "orderedEofAcked",
    ] {
        let mut r = report();
        r[key] = json!(false);
        assert!(validate(&r).is_err());
    }
    let mut r = report();
    r["limits"] = json!([]);
    assert!(validate(&r).is_err());
}
