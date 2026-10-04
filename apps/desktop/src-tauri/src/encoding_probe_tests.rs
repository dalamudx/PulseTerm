#![allow(clippy::unwrap_used)]
use super::*;
fn evidence() -> EncodingEvidence {
    EncodingEvidence {
        generation: "2".into(),
        source_bytes: "89".into(),
        utf8_bytes: "91".into(),
        revision: "20".into(),
        digest: "9e1323ca657a4985".into(),
        frames: 100,
        peak_credit_bytes: 64,
        replacements: 1,
        reverse_input_checked: true,
        source_only_marker: true,
        dom_rows_checked: 12,
        history_checked: true,
        callback_ack_checked: true,
    }
}
#[test]
fn gbk_evidence_requires_both_distinct_coordinates_and_actual_dom_callback_evidence() {
    let mut e = evidence();
    assert!(e.validate().is_ok());
    e.utf8_bytes = "89".into();
    assert!(e.validate().is_err());
    e.utf8_bytes = "91".into();
    e.source_bytes = "91".into();
    assert!(e.validate().is_err());
    e.source_bytes = "89".into();
    e.dom_rows_checked = 0;
    assert!(e.validate().is_err());
    e.dom_rows_checked = 12;
    e.callback_ack_checked = false;
    assert!(e.validate().is_err());
    e.callback_ack_checked = true;
    e.history_checked = false;
    assert!(e.validate().is_err());
    e.history_checked = true;
    e.source_only_marker = false;
    assert!(e.validate().is_err());
    e.source_only_marker = true;
    e.reverse_input_checked = false;
    assert!(e.validate().is_err());
}
#[test]
fn gbk_evidence_credit_replacement_and_canonical_counters_are_bounded() {
    let mut e = evidence();
    e.peak_credit_bytes = 65;
    assert!(e.validate().is_err());
    e.peak_credit_bytes = 64;
    e.replacements = 0;
    assert!(e.validate().is_err());
    e.replacements = 1;
    e.frames = 16385;
    assert!(e.validate().is_err());
    e.frames = 100;
    e.source_bytes = "089".into();
    assert!(e.validate().is_err());
    e.source_bytes = "89".into();
    e.generation = "0".into();
    assert!(e.validate().is_err());
    e.generation = "2".into();
    e.digest = "G".repeat(16);
    assert!(e.validate().is_err());
}
