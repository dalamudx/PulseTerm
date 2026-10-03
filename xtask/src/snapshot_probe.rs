//! Strict restore gate: equal initial screens alone do not prove state restore.
use anyhow::{Result, ensure};
use pulse_core::TermSize;
use pulse_term::TerminalProbe;
use serde_json::json;
pub fn run() -> Result<()> {
    let size = TermSize::new(32, 8)?;
    let cases = [
        ("ordinary-text", "hello", " world"),
        ("pending-wrap", "12345678901234567890123456789012", "Z"),
        ("inactive-main-buffer", "main\x1b[?1049halt", "\x1b[?1049lZ"),
        ("saved-cursor", "\x1b[2;3H\x1b7\x1b[5;8H", "\x1b8Z"),
        ("scroll-region", "\x1b[2;4r\x1b[4;1Hbottom", "\r\nZ"),
        ("insert-mode", "ABCDE\r\x1b[4h", "Z"),
        ("active-charset", "\x1b(0", "qqq"),
        ("partial-csi", "\x1b[3;", "4HZ"),
        ("partial-utf8", "", "Z"),
    ];
    let mut results = Vec::new();
    for (name, prefix, suffix) in cases {
        let mut original = TerminalProbe::new(size, 128);
        original.feed(prefix.as_bytes());
        if name == "partial-utf8" {
            original.feed(&[0xf0, 0x9f]);
        }
        let replay = original.screen_replay();
        let mut restored = TerminalProbe::new(size, 128);
        restored.feed(replay.as_bytes());
        let initial_matches = restored.screen() == original.screen();
        original.take_replies();
        restored.take_replies();
        if name == "partial-utf8" {
            original.feed(&[0x8c, 0x8d]);
            restored.feed(&[0x8c, 0x8d]);
        }
        original.feed(suffix.as_bytes());
        restored.feed(suffix.as_bytes());
        original.finish_input();
        restored.finish_input();
        let cells_match = original.screen() == restored.screen();
        let queries = b"\x1b[6n\x1b[4$p\x1b[?6$p";
        original.feed(queries);
        restored.feed(queries);
        let original_replies = original.take_replies();
        let restored_replies = restored.take_replies();
        let replies_match = original_replies == restored_replies;
        let passed = initial_matches && cells_match && replies_match;
        println!(
            "{} {name}: initial={initial_matches} continuation={cells_match} replies={replies_match}",
            if passed { "PASS" } else { "FAIL" }
        );
        results.push(json!({"name":name,"passed":passed,"initialMatches":initial_matches,"cellsMatch":cells_match,"repliesMatch":replies_match,"originalReplies":original_replies,"restoredReplies":restored_replies,"expected":original.screen(),"actual":restored.screen()}));
    }
    let passed = results.iter().all(|r| r["passed"] == true);
    std::fs::create_dir_all("target/probes")?;
    std::fs::write(
        "target/probes/snapshot.json",
        serde_json::to_vec_pretty(
            &json!({"schemaVersion":1,"acceptancePassed":passed,"cases":results}),
        )?,
    )?;
    ensure!(
        passed,
        "complete restore gate failed; visible-screen replay must not be used for attach"
    );
    Ok(())
}
