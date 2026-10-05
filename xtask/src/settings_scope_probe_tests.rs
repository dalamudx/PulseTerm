use super::*;
fn receipts() -> [Value; 5] {
    let mut receipts = std::array::from_fn(
        |i| json!({"mode":MODES[i],"pid":100+i,"workerJoined":true,"receipt":{}}),
    );
    receipts[0]["receipt"] = json!({"initialized":true});
    receipts[1]["receipt"] = json!({"edited":true});
    receipts[2]["receipt"] = json!({"edited":true});
    receipts[3]["receipt"] = json!({"scopeIsolation":true,"concurrentEditsPreserved":true,"userFileChecked":true,"perFieldSourcesChecked":true,"contextSwitchChecked":true,"lateCandidateRejected":true,"contextOwnerCloseChecked":true,"registryWindowAuthorizationChecked":true,"registryRefreshChecked":true,"registryTransferChecked":true,"groupRevision":3,"hostRevision":1});
    receipts[4]["receipt"] = json!({"abaRejected":true,"rowPreserved":true});
    receipts
}
#[test]
fn source_receipts_cannot_be_truthy_missing_or_substituted() -> Result<()> {
    let good = receipts();
    verify(&good)?;
    for (index, key) in [
        (0, "initialized"),
        (1, "edited"),
        (2, "edited"),
        (3, "scopeIsolation"),
        (3, "concurrentEditsPreserved"),
        (3, "userFileChecked"),
        (3, "perFieldSourcesChecked"),
        (3, "contextSwitchChecked"),
        (3, "lateCandidateRejected"),
        (3, "contextOwnerCloseChecked"),
        (3, "registryWindowAuthorizationChecked"),
        (3, "registryRefreshChecked"),
        (3, "registryTransferChecked"),
        (4, "abaRejected"),
        (4, "rowPreserved"),
    ] {
        for value in [Value::Null, json!("true"), json!(1), json!(false)] {
            let mut bad = good.clone();
            bad[index]["receipt"][key] = value;
            assert!(verify(&bad).is_err());
        }
    }
    Ok(())
}
#[test]
fn processes_modes_and_join_receipts_require_distinct_fixed_identities() -> Result<()> {
    let good = receipts();
    verify(&good)?;
    for index in 0..5 {
        for (key, value) in [
            ("mode", json!("read")),
            ("workerJoined", json!("true")),
            ("pid", json!(0)),
            ("pid", json!(u64::MAX)),
        ] {
            if key == "mode" && index == 3 {
                continue;
            }
            let mut bad = good.clone();
            bad[index][key] = value;
            assert!(verify(&bad).is_err());
        }
    }
    let mut bad = good.clone();
    bad[4]["pid"] = bad[0]["pid"].clone();
    assert!(verify(&bad).is_err());
    Ok(())
}
#[test]
fn old_or_string_revision_does_not_substitute_atomic_edit_evidence() -> Result<()> {
    for (key, value) in [
        ("groupRevision", json!(2)),
        ("groupRevision", json!("3")),
        ("hostRevision", json!(3)),
        ("hostRevision", Value::Null),
    ] {
        let mut bad = receipts();
        bad[3]["receipt"][key] = value;
        assert!(verify(&bad).is_err());
    }
    Ok(())
}
