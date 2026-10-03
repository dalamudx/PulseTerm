//! Shared, finite protocol-query cases for the authoritative Rust responder.
use anyhow::{Context, Result, bail};
use pulse_core::TermSize;
use pulse_term::TerminalProbe;
use serde_json::{Value, json};

pub fn run(size: TermSize) -> Result<Vec<Value>> {
    let corpus: Value =
        serde_json::from_slice(&std::fs::read("tests/fixtures/terminal/queries.json")?)?;
    if corpus["schema_version"] != 1
        || corpus["columns"] != size.columns()
        || corpus["rows"] != size.rows()
    {
        bail!("query corpus schema or dimensions do not match probe");
    }
    let cases = corpus["cases"].as_array().context("missing query cases")?;
    if cases.is_empty() {
        bail!("empty query corpus");
    }
    let mut results = Vec::with_capacity(cases.len());
    for case in cases {
        let name = case["name"].as_str().context("query name")?;
        let input = case["input"].as_str().context("query input")?;
        let expected: Vec<&str> = case["backendReplies"]
            .as_array()
            .context("query replies")?
            .iter()
            .map(|reply| reply.as_str().context("non-string reply"))
            .collect::<Result<_>>()?;
        let mut whole = TerminalProbe::new(size, 128);
        whole.feed(input.as_bytes());
        whole.finish_input();
        let actual = whole.take_replies();
        if actual != expected {
            bail!("query {name}: expected {expected:?}, got {actual:?}");
        }
        let screen = whole.screen();
        let mut partitions = 0;
        for length in 1..=input.len().max(1) {
            let mut chunked = TerminalProbe::new(size, 128);
            let mut replies = Vec::new();
            for chunk in input.as_bytes().chunks(length) {
                chunked.feed(chunk);
                replies.extend(chunked.take_replies());
            }
            chunked.finish_input();
            replies.extend(chunked.take_replies());
            if replies != expected || chunked.screen() != screen {
                bail!("query {name} differs with {length}-byte chunks");
            }
            partitions += 1;
        }
        results.push(json!({"name":name, "input":input, "backend_replies":actual,
            "expected_replies":expected, "expected":screen, "chunk_sizes_checked":partitions}));
    }
    Ok(results)
}
