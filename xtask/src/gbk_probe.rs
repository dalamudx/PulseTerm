//! Test-only legacy charset samples; no automatic detection or production encoding pipeline.
use anyhow::{Result, ensure};
use encoding_rs::{DecoderResult, GBK};
use serde_json::{Value, json};
const TEXT: &str = "\x1b[2J\x1b[H中文网络设备\r\n输入命令\x1b[6n";
pub(crate) fn encode_input(text: &str) -> Result<Vec<u8>> {
    ensure!(text.len() <= 4096, "encoding input bound");
    let (encoded, _, lossy) = GBK.encode(text);
    ensure!(
        !lossy,
        "unrepresentable GBK input must be refused, not numeric-entity substituted"
    );
    Ok(encoded.into_owned())
}
pub(crate) fn fixture() -> Result<Vec<u8>> {
    encode_input(TEXT)
}
fn decode_chunks(raw: &[u8], chunk: usize) -> Result<String> {
    ensure!(raw.len() <= 4096 && chunk > 0, "GBK sample bounds");
    let mut decoder = GBK.new_decoder_without_bom_handling();
    let mut output = String::with_capacity(16384);
    for bytes in raw.chunks(chunk) {
        let (result, consumed) =
            decoder.decode_to_string_without_replacement(bytes, &mut output, false);
        ensure!(
            matches!(result, DecoderResult::InputEmpty) && consumed == bytes.len(),
            "malformed GBK or bounded output overflow"
        );
    }
    let (result, consumed) = decoder.decode_to_string_without_replacement(&[], &mut output, true);
    ensure!(
        matches!(result, DecoderResult::InputEmpty) && consumed == 0,
        "GBK incomplete EOF not accepted"
    );
    Ok(output)
}
pub(crate) fn verify_network_reply(raw: &[u8]) -> Result<Value> {
    ensure!(raw == fixture()?.as_slice(), "SSH changed legacy GBK bytes");
    let size = pulse_core::TermSize::new(80, 24)?;
    let mut reference = pulse_term::TerminalProbe::new(size, 128);
    reference.feed(TEXT.as_bytes());
    reference.finish_input();
    let expected = reference.screen();
    let replies = reference.take_replies();
    ensure!(!replies.is_empty(), "query fixture did not answer");
    for chunk in 1..=raw.len() {
        let decoded = decode_chunks(raw, chunk)?;
        ensure!(decoded == TEXT, "GBK arbitrary split changed text");
        let mut parsed = pulse_term::TerminalProbe::new(size, 128);
        parsed.feed(decoded.as_bytes());
        parsed.finish_input();
        ensure!(
            parsed.screen().cells == expected.cells && parsed.take_replies() == replies,
            "UTF8 normalized backend parse/query mismatch"
        );
    }
    ensure!(
        decode_chunks(&[0x81], 1).is_err() && decode_chunks(&[0xff], 1).is_err(),
        "invalid/incomplete GBK silently replaced"
    );
    ensure!(
        encode_input("🌍").is_err(),
        "unrepresentable emoji silently substituted"
    );
    Ok(
        json!({"schemaVersion":1,"binarySshBytesExact":true,"rawBytes":raw.len(),"utf8Bytes":TEXT.len(),"allChunkSizes":raw.len(),"scalarAndAnsiTextExact":true,"backendCellsAndQueryRepliesExact":true,"malformedAndEofRefused":true,"unrepresentableInputRefused":true,"limits":["Synthetic GBK echo, not a real network device or production encoder","No encoding auto-detection, Zmodem binary preclassification, actor original-byte boundary/reconnect or charset UI integration"]}),
    )
}
pub(crate) fn validate_report(r: &Value) -> Result<()> {
    ensure!(
        r["schemaVersion"] == 1
            && r["rawBytes"]
                .as_u64()
                .is_some_and(|n| (1..=4096).contains(&n))
            && r["utf8Bytes"]
                .as_u64()
                .is_some_and(|n| (1..=4096).contains(&n))
            && r["allChunkSizes"] == r["rawBytes"],
        "GBK sample byte/chunk evidence"
    );
    for field in [
        "binarySshBytesExact",
        "scalarAndAnsiTextExact",
        "backendCellsAndQueryRepliesExact",
        "malformedAndEofRefused",
        "unrepresentableInputRefused",
    ] {
        ensure!(r[field] == true, "missing GBK evidence");
    }
    ensure!(
        r["limits"].as_array().is_some_and(|a| !a.is_empty()),
        "GBK limits missing"
    );
    Ok(())
}
#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used)]
    use super::*;
    #[test]
    fn every_gbk_split_keeps_chinese_ansi_and_query_bytes() {
        let raw = fixture().unwrap();
        validate_report(&verify_network_reply(&raw).unwrap()).unwrap();
        assert_ne!(raw.len(), TEXT.len());
    }
    #[test]
    fn malformed_eof_and_unrepresentable_input_fail_instead_of_replacement() {
        assert!(decode_chunks(&[0x81], 1).is_err());
        assert!(decode_chunks(&[0xff], 1).is_err());
        assert!(encode_input("🌍").is_err());
        assert!(decode_chunks(b"ok", 0).is_err());
        assert!(encode_input(&"a".repeat(4097)).is_err());
    }
    #[test]
    fn flags_alone_cannot_claim_gbk_network_or_parser_parity() {
        assert!(validate_report(&json!({"schemaVersion":1,"completed":true})).is_err());
    }
}
