#![allow(clippy::unwrap_used)]
use super::*;
fn release(timeout: bool) -> serde_json::Value {
    json!({"proxyOrder":[],"proxyRelaysJoined":true,"kind":if timeout{"ack-timeout"}else{"detach"},"sourceBytes":TOTAL,"utf8Bytes":TOTAL/6*7,"actorControlResponsive":true,"viewRevokedOnly":true,"sharedControlRecovered":true,"remoteResumed":true,"actorEndedNormally":true,"fullActorScreenExact":true,"connectionsListenerAndProducerJoined":true})
}
fn report() -> serde_json::Value {
    json!({"schemaVersion":1,"proxyOrder":[],"proxyRelaysJoined":true,"sourceBytes":TOTAL,"utf8Bytes":TOTAL/6*7,"receiveWindow":32768,"receiveBufferMessages":2,"maxPacket":8192,"peakCreditBytes":8220,"pausedActorSource":57338,"pausedBridgeRead":114660,"pausedRemoteProduced":155610,"actorControlResponsive":true,"rawAndNormalizedBytesExact":true,"actorAndReaderAndRemoteProducerStopped":true,"oldConnectionControlBlocked":true,"separateConnectionResponsive":true,"sameConnectionControlRecoveredAfterAck":true,"actorScreenExact":true,"realSshEofAndClose":true,"orderedEofAcked":true,"allConnectionsAndListenerJoined":true,"detached":release(false),"timedOut":release(true),"limits":["known HOL is not release approval"]})
}
#[test]
fn pause_must_stop_actual_remote_producer_not_just_actor() {
    let mut r = report();
    assert!(validate(&r, false).is_ok());
    r["pausedRemoteProduced"] = json!(TOTAL);
    assert!(validate(&r, false).is_err());
    r["pausedRemoteProduced"] = json!(155610);
    r["pausedBridgeRead"] = json!(1024 * 1024 + 1);
    assert!(validate(&r, false).is_err());
    assert!(validate(&json!({"completed":true}), false).is_err());
}
#[test]
fn hol_limitation_and_all_eof_release_observations_are_required() {
    for key in [
        "actorControlResponsive",
        "rawAndNormalizedBytesExact",
        "actorAndReaderAndRemoteProducerStopped",
        "oldConnectionControlBlocked",
        "separateConnectionResponsive",
        "sameConnectionControlRecoveredAfterAck",
        "actorScreenExact",
        "realSshEofAndClose",
        "orderedEofAcked",
        "allConnectionsAndListenerJoined",
    ] {
        let mut r = report();
        r[key] = json!(false);
        assert!(validate(&r, false).is_err());
    }
    let mut r = report();
    r["peakCreditBytes"] = json!(8221);
    assert!(validate(&r, false).is_err());
}
#[test]
fn requested_proxy_path_cannot_be_substituted_by_direct_release_evidence() {
    let mut r = report();
    assert!(validate(&r, false).is_ok());
    assert!(validate(&r, true).is_err());
    r["proxyOrder"] = proxy_order(true);
    r["detached"]["proxyOrder"] = proxy_order(true);
    r["timedOut"]["proxyOrder"] = proxy_order(true);
    assert!(validate(&r, true).is_ok());
    r["detached"]["proxyOrder"] = json!([]);
    assert!(validate(&r, true).is_err());
    r["detached"]["proxyOrder"] = proxy_order(true);
    r["proxyRelaysJoined"] = json!(false);
    assert!(validate(&r, true).is_err());
}

#[test]
fn release_kind_normal_eof_screen_and_cleanup_cannot_be_substituted() {
    let mut r = release(true);
    assert!(cancel::validate(&r, true, false).is_ok());
    assert!(cancel::validate(&r, false, false).is_err());
    for key in [
        "actorControlResponsive",
        "viewRevokedOnly",
        "sharedControlRecovered",
        "remoteResumed",
        "actorEndedNormally",
        "fullActorScreenExact",
        "connectionsListenerAndProducerJoined",
    ] {
        r = release(true);
        r[key] = json!(false);
        assert!(cancel::validate(&r, true, false).is_err());
    }
    let mut outer = report();
    outer["timedOut"] = release(false);
    assert!(validate(&outer, false).is_err());
}
