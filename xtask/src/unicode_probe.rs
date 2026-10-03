//! Exact Rust projections and arbitrary-byte partitions for a Unicode corpus.
use anyhow::{Context, Result, ensure};
use pulse_core::TermSize;
use pulse_term::TerminalProbe;
use serde_json::{Value, json};
pub fn run(size: TermSize) -> Result<Vec<Value>> {
    let corpus: Value =
        serde_json::from_slice(&std::fs::read("tests/fixtures/terminal/unicode.json")?)?;
    ensure!(corpus["schema_version"] == 1, "Unicode corpus version");
    let mut results = Vec::new();
    for case in corpus["cases"].as_array().context("Unicode cases")? {
        let input = case["input"].as_str().context("Unicode input")?;
        let mut terminal = TerminalProbe::new(size, 128);
        terminal.feed(input.as_bytes());
        terminal.finish_input();
        let expected = terminal.screen();
        for chunk_size in 1..=input.len().max(1) {
            let mut chunked = TerminalProbe::new(size, 128);
            for bytes in input.as_bytes().chunks(chunk_size) {
                chunked.feed(bytes);
            }
            chunked.finish_input();
            ensure!(
                chunked.screen() == expected,
                "Rust Unicode partitions differ: {}",
                case["name"]
            );
        }
        results.push(json!({"name":case["name"], "input":input, "required":case["required"], "expected":expected, "chunk_sizes_checked":input.len().max(1)}));
    }
    ensure!(!results.is_empty(), "empty Unicode corpus");
    Ok(results)
}
