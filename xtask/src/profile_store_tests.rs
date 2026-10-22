use super::*;
fn receipts() -> [Value; 5] {
    [
        json!({"mode":"write","pid":1,"workerJoined":true,"receipt":{"written":true}}),
        json!({"mode":"edit-fish","pid":2,"workerJoined":true,"receipt":{"edited":true}}),
        json!({"mode":"edit-sh","pid":3,"workerJoined":true,"receipt":{"edited":true}}),
        json!({"mode":"conflict","pid":4,"workerJoined":true,"receipt":{"staleSnapshotRejected":true,"rowPreserved":true}}),
        json!({"mode":"read","pid":5,"workerJoined":true,"receipt":{"hiddenRestored":true,"deletedSuppressed":true,"delayedDiscoveryRetained":true,"atomicEditsPreserved":true}}),
    ]
}
fn check(v: &[Value; 5]) -> Result<()> {
    verify(&v[0], &v[1], &v[2], &v[3], &v[4])
}
#[test]
fn independent_processes_modes_and_join_receipts_are_mandatory() -> Result<()> {
    let base = receipts();
    check(&base)?;
    for index in 0..5 {
        for field in ["workerJoined", "pid", "mode"] {
            let mut v = base.clone();
            v[index][field] = Value::Null;
            assert!(check(&v).is_err());
        }
    }
    let mut v = base.clone();
    v[4]["pid"] = json!(1);
    assert!(check(&v).is_err());
    let mut v = base.clone();
    v[3]["pid"] = json!(0);
    assert!(check(&v).is_err());
    let mut v = base.clone();
    v[3]["pid"] = json!(u64::MAX);
    assert!(check(&v).is_err());
    let mut v = base;
    v[1]["mode"] = json!("edit-sh");
    assert!(check(&v).is_err());
    Ok(())
}
#[test]
fn edit_conflict_and_final_preservation_evidence_require_literal_true() -> Result<()> {
    let base = receipts();
    check(&base)?;
    for (index, key) in [
        (0, "written"),
        (1, "edited"),
        (2, "edited"),
        (3, "staleSnapshotRejected"),
        (3, "rowPreserved"),
        (4, "hiddenRestored"),
        (4, "deletedSuppressed"),
        (4, "delayedDiscoveryRetained"),
        (4, "atomicEditsPreserved"),
    ] {
        for bad in [Value::Null, json!(1), json!("true"), json!(false)] {
            let mut v = base.clone();
            v[index]["receipt"][key] = bad;
            assert!(check(&v).is_err());
        }
    }
    Ok(())
}

#[cfg(unix)]
#[tokio::test]
async fn native_helper_diagnostics_preserve_failure_and_confirm_timeout_join() -> Result<()> {
    let dir = tempfile::tempdir()?;
    let mut success = tokio::process::Command::new("/bin/sh");
    success.args(["-c", "printf receipt; printf stage >&2"]);
    let output = helper_output(&mut success, "success", dir.path(), Duration::from_secs(5)).await?;
    assert_eq!(output.stdout, b"receipt");
    assert_eq!(output.stderr, b"stage");
    let mut failure = tokio::process::Command::new("/bin/sh");
    failure.args(["-c", "printf refused >&2; exit 7"]);
    assert!(
        helper_output(&mut failure, "failure", dir.path(), Duration::from_secs(5))
            .await
            .is_err()
    );
    let failed: Value = serde_json::from_slice(&std::fs::read(dir.path().join("failure.json"))?)?;
    assert_eq!(failed["exitCode"], 7);
    assert_eq!(failed["processJoined"], true);
    assert_eq!(
        std::fs::read(dir.path().join("failure-stderr.log"))?,
        b"refused"
    );
    let mut stalled = tokio::process::Command::new("/bin/sh");
    stalled.args(["-c", "exec sleep 30"]);
    assert!(
        tokio::time::timeout(
            Duration::from_secs(5),
            helper_output(
                &mut stalled,
                "stalled",
                dir.path(),
                Duration::from_millis(20)
            ),
        )
        .await?
        .is_err()
    );
    let timed: Value = serde_json::from_slice(&std::fs::read(dir.path().join("stalled.json"))?)?;
    assert_eq!(timed["timedOut"], true);
    assert_eq!(timed["processJoined"], true);
    assert!(dir.path().join("success.json").is_file());
    Ok(())
}
