#![allow(clippy::unwrap_used)]
use super::*;
use serde_json::json;
fn report() -> Value {
    let row = json!({"native":{"native":{"nativeVisibleRegistered":true,"serverToNativeUs":10000},"pageLoadedUs":350000,"nativeDestroyed":true},"client":{"requestThroughNativeReceiptUs":12000,"nativeReceiptMatched":true}});
    json!({"schemaVersion":1,"completed":true,"mainPageReadyBeforeClient":true,"badTokenCreatedNoWindow":true,"clientsListenerWindowsAndDirectoryCleaned":true,"samples":[row.clone(),row],"limits":["native registered is not page ready or first paint"]})
}
#[test]
fn native_registration_ack_does_not_need_to_wait_for_page_load() {
    let r = report();
    assert!(validate(&r).is_ok());
    assert!(
        r["samples"][0]["client"]["requestThroughNativeReceiptUs"]
            .as_u64()
            .unwrap()
            < r["samples"][0]["native"]["pageLoadedUs"].as_u64().unwrap()
    );
}
#[test]
fn fake_or_inverted_native_receipts_and_cleanup_are_rejected() {
    let mut r = report();
    r["samples"][0]["native"]["native"]["nativeVisibleRegistered"] = json!(false);
    assert!(validate(&r).is_err());
    r = report();
    r["samples"][0]["native"]["pageLoadedUs"] = json!(1);
    assert!(validate(&r).is_err());
    r = report();
    r["samples"][0]["client"]["requestThroughNativeReceiptUs"] = json!(1);
    assert!(validate(&r).is_err());
    r = report();
    r["clientsListenerWindowsAndDirectoryCleaned"] = json!(false);
    assert!(validate(&r).is_err());
    r = report();
    r["samples"] = json!([r["samples"][0].clone()]);
    assert!(validate(&r).is_err());
    assert!(validate(&json!({"completed":true})).is_err());
}
