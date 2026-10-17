use super::*;
fn valid() -> Receipt {
    Receipt {
        schema: 3,
        pid: 7,
        phase: Phase::Committed,
        slot_checked: true,
        roundtrip_checked: true,
        background_io_checked: true,
        context_checked: true,
    }
}
#[test]
fn exact_phase_pid_and_document_receipt_is_required() -> Result<()> {
    let text = format!("{}\n", serde_json::to_string(&valid())?);
    parse_receipt(&text, 7, Phase::Committed)?;
    assert!(parse_receipt(&text, 8, Phase::Committed).is_err());
    assert!(parse_receipt(&text, 7, Phase::Read).is_err());
    assert!(parse_receipt(&text, 0, Phase::Committed).is_err());
    Ok(())
}
#[test]
fn missing_unknown_oversized_duplicate_and_future_are_not_receipts() -> Result<()> {
    let baseline = serde_json::to_value(valid())?;
    for (key, value) in [
        ("schema", serde_json::json!(4)),
        ("schema", serde_json::json!(2)),
        ("schema", serde_json::json!(1)),
        ("slot_checked", serde_json::json!(false)),
        ("roundtrip_checked", serde_json::json!(false)),
        ("background_io_checked", serde_json::json!(false)),
        ("context_checked", serde_json::json!(false)),
        ("extra", serde_json::json!(true)),
        ("phase", serde_json::json!("unknown")),
    ] {
        let mut bad = baseline.clone();
        bad[key] = value;
        assert!(parse_receipt(&format!("{bad}\n"), 7, Phase::Committed).is_err());
    }
    for key in [
        "schema",
        "pid",
        "phase",
        "slot_checked",
        "roundtrip_checked",
        "background_io_checked",
        "context_checked",
    ] {
        let mut bad = baseline.clone();
        bad.as_object_mut()
            .ok_or_else(|| anyhow::anyhow!("fixture is not an object"))?
            .remove(key);
        assert!(parse_receipt(&format!("{bad}\n"), 7, Phase::Committed).is_err());
    }
    let text = serde_json::to_string(&valid())?;
    let duplicate = format!("{{\"schema\":3,{}\n", &text[1..]);
    assert!(parse_receipt(&duplicate, 7, Phase::Committed).is_err());
    assert!(parse_receipt(&"x".repeat(LIMIT + 1), 7, Phase::Committed).is_err());
    assert!(parse_receipt(&text, 7, Phase::Committed).is_err());
    Ok(())
}
