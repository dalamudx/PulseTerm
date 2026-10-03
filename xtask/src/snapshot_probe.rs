//! Complete-restore probe. Rust side: structural fingerprint after restore and
//! after continuation. Writes cases for the xterm.js side
//! (ui/scripts/snapshot-probe.ts), which checks the real display consumer.
use anyhow::{Result, ensure};
use pulse_core::TermSize;
use pulse_term::{RestoreGap, TerminalProbe};
use serde_json::{Value, json};

const TOKENS: &[&str] = &[
    "a",
    "Z",
    "q",
    "中",
    "é",
    "e\u{301}",
    "\r",
    "\n",
    "\t",
    "\x08",
    "12345678901234567890123456789012",
    "\x1b[2;5r",
    "\x1b[r",
    "\x1b[?6h",
    "\x1b[?6l",
    "\x1b[3;7H",
    "\x1b[H",
    "\x1b[8;30H",
    "\x1b7",
    "\x1b8",
    "\x1b(0",
    "\x1b(B",
    "\x1b)0",
    "\x0e",
    "\x0f",
    "\x1b[4h",
    "\x1b[4l",
    "\x1b[20h",
    "\x1b[20l",
    "\x1b[?7l",
    "\x1b[?7h",
    "\x1bH",
    "\x1b[g",
    "\x1b[3g",
    "\x1b[5G",
    "\x1b[1;31m",
    "\x1b[4m",
    "\x1b[0m",
    "\x1b[38;2;1;2;3;48;5;200m",
    "\x1b[7m",
    "\x1b]4;1;#123456\x07",
    "\x1b]104\x07",
    "\x1b[?1049h",
    "\x1b[?1049l",
    "\x1b[?25l",
    "\x1b[?2004h",
    "\x1b[?1002h",
    "\x1b[?1h",
    "\x1b=",
    "\x1b[2J",
    "\x1b[K",
    "\x1b[L",
    "\x1b[M",
    "\x1b[P",
    "\x1b[@",
    "\x1bM",
    "\x1bD",
    "\x1bE",
    "\x1b[3S",
    "\x1b[2T",
    "\x1b[3X",
    "\x1b[3;",
];

struct Case {
    name: String,
    kind: &'static str,
    prefix: Vec<u8>,
    suffix: Vec<u8>,
    resized: Option<TermSize>,
}

fn named() -> Vec<Case> {
    let mut cases: Vec<(&str, Vec<u8>, Vec<u8>)> = vec![
        ("ordinary-text", b"hello".to_vec(), b" world".to_vec()),
        (
            "pending-wrap",
            b"12345678901234567890123456789012".to_vec(),
            b"Z".to_vec(),
        ),
        (
            "wide-pending-wrap",
            "123456789012345678901234567890中".into(),
            b"Z".to_vec(),
        ),
        (
            "inactive-main-buffer",
            b"main\x1b[?1049halt".to_vec(),
            b"\x1b[?1049lZ".to_vec(),
        ),
        (
            "saved-cursor",
            b"\x1b[2;3H\x1b[1;32m\x1b(0\x1b7\x1b[0m\x1b(B\x1b[5;8H".to_vec(),
            b"\x1b8qZ".to_vec(),
        ),
        (
            "scroll-region",
            b"\x1b[2;4r\x1b[4;1Hbottom".to_vec(),
            b"\r\nZ\r\nY".to_vec(),
        ),
        (
            "origin-region",
            b"\x1b[2;5r\x1b[?6h\x1b[2;3H".to_vec(),
            b"Z\x1b[1;1HA\r\n\r\n\r\n\r\nW".to_vec(),
        ),
        ("insert-mode", b"ABCDE\r\x1b[4h".to_vec(), b"Z".to_vec()),
        ("designated-charset", b"\x1b(0".to_vec(), b"qqq".to_vec()),
        (
            "shift-out-g1",
            b"\x1b)0\x0e".to_vec(),
            b"qqq\x0fqq".to_vec(),
        ),
        ("pen-without-text", b"\x1b[1;4;31m".to_vec(), b"Z".to_vec()),
        (
            "tab-stops",
            b"\x1b[3g\x1b[1;5H\x1bH\x1b[1;12H\x1bH\r".to_vec(),
            b"\tA\tB".to_vec(),
        ),
        (
            "colour-override",
            b"\x1b]4;1;#112233\x07".to_vec(),
            b"\x1b[31mZ".to_vec(),
        ),
        (
            "input-modes",
            b"\x1b[?1000h\x1b[?1006h\x1b[?2004h\x1b[?1h\x1b=".to_vec(),
            b"Z".to_vec(),
        ),
        (
            "hyperlink-pen",
            b"\x1b]8;;https://example.com\x1b\\".to_vec(),
            b"link\x1b]8;;\x1b\\ x".to_vec(),
        ),
        ("partial-csi", b"\x1b[3;".to_vec(), b"4HZ".to_vec()),
        ("partial-osc", b"\x1b]0;tit".to_vec(), b"le\x07Z".to_vec()),
        (
            "partial-utf8",
            vec![b'X', 0xf0, 0x9f],
            vec![0x8c, 0x8d, b'Z'],
        ),
        (
            "sync-pending",
            b"\x1b[?2026hbuffered".to_vec(),
            b"\x1b[?2026lZ".to_vec(),
        ),
    ];
    cases
        .drain(..)
        .map(|(name, prefix, suffix)| Case {
            name: name.into(),
            kind: "named",
            prefix,
            suffix,
            resized: None,
        })
        .collect()
}

fn alternate_cases() -> Result<Vec<Case>> {
    let prefixes = [
        (
            "alt-exit-attributes",
            "MAIN 中\x1b[31m\x1b7\x1b[?1049hALT",
            "\x1b[?1049l!\x1b8?",
        ),
        (
            "alt-reenter-saved",
            "MAIN\x1b[?1049h\x1b[3;4H\x1b[34m\x1b7\x1b[?1049l",
            "\x1b[?1049h\x1b8ALT\x1b[?1049l!",
        ),
        (
            "alt-link-inheritance",
            "\x1b]8;;http://x\x1b\\\x1b[?1049h\r",
            "X\x1b[?1049lY",
        ),
        (
            "alt-hidden-reflow",
            "0123456789012345678901234567890123456789\r\nSECOND\x1b[?1049hALT",
            "\x1b[?1049l!",
        ),
    ];
    let mut cases = Vec::new();
    for (name, prefix, suffix) in prefixes {
        for (variant, resized) in [
            ("original", None),
            ("shrink", Some(TermSize::new(20, 6)?)),
            ("grow", Some(TermSize::new(44, 12)?)),
        ] {
            cases.push(Case {
                name: format!("{name}-{variant}"),
                kind: "named",
                prefix: prefix.as_bytes().to_vec(),
                suffix: suffix.as_bytes().to_vec(),
                resized,
            });
        }
    }
    Ok(cases)
}

fn random(count: usize) -> Vec<Case> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut next = |bound: usize| {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        (state % bound as u64) as usize
    };
    (0..count)
        .map(|index| {
            let mut take = |max: usize| -> Vec<u8> {
                let len = next(max);
                (0..len)
                    .flat_map(|_| TOKENS[next(TOKENS.len())].bytes())
                    .collect()
            };
            let prefix = take(60);
            let suffix = take(30);
            Case {
                name: format!("random-{index}"),
                kind: "random",
                prefix,
                suffix,
                resized: None,
            }
        })
        .collect()
}

fn first_diff(expected: &str, actual: &str) -> Option<String> {
    expected
        .lines()
        .zip(actual.lines())
        .enumerate()
        .find(|(_, (a, b))| a != b)
        .map(|(line, (a, b))| {
            let (x, y): (Vec<&str>, Vec<&str>) = (a.split('|').collect(), b.split('|').collect());
            match x.iter().zip(&y).position(|(p, q)| p != q) {
                Some(col) if x.len() > 2 => format!(
                    "line {line} col {col}: expected {} actual {}",
                    x[col], y[col]
                ),
                _ => format!("line {line}: expected {a} actual {b}"),
            }
        })
}

pub fn run() -> Result<()> {
    let size = TermSize::new(32, 8)?;
    let mut results = Vec::new();
    let mut strict_failures = Vec::new();
    for case in named()
        .into_iter()
        .chain(alternate_cases()?)
        .chain(random(600))
    {
        let mut original = TerminalProbe::new(size, 128);
        original.feed(&case.prefix);
        if let Some(resized) = case.resized {
            original.resize(resized);
        }
        let restore_size = case.resized.unwrap_or(size);
        let restore = original.restore_sequence();
        let mut continued = TerminalProbe::new(size, 128);
        continued.feed(&case.prefix);
        if let Some(resized) = case.resized {
            continued.resize(resized);
        }
        continued.feed(&case.suffix);
        continued.finish_input();
        let expected = continued.screen();
        let (restore_bytes, gap, main_missing, approximations, rust_exact) = match &restore {
            Err(gap) => (
                Value::Null,
                Some(match gap {
                    RestoreGap::OutputTooLarge => "output-too-large",
                    RestoreGap::SyncPending => "sync-pending",
                    RestoreGap::PendingSequenceTooLong => "pending-too-long",
                    RestoreGap::InactiveCursorDivergence => "inactive-cursor-divergence",
                    RestoreGap::NonEdgePendingWrap => "nonedge-pending-wrap",
                }),
                false,
                Vec::new(),
                None,
            ),
            Ok(restore) => {
                let mut restored = TerminalProbe::new(restore_size, 128);
                restored.feed(&restore.bytes);
                let after_restore = first_diff(
                    &original.restore_fingerprint(),
                    &restored.restore_fingerprint(),
                );
                let mut exact = after_restore.is_none() && restored.take_replies().is_empty();
                restored.feed(&case.suffix);
                restored.finish_input();
                let after_suffix = first_diff(
                    &continued.restore_fingerprint(),
                    &restored.restore_fingerprint(),
                );
                exact &= after_suffix.is_none();
                if !exact
                    && restore.approximations.is_empty()
                    && !restore.main_screen_missing
                    && std::env::var_os("PULSE_RESTORE_DEBUG").is_some()
                {
                    eprintln!(
                        "{}: restore {after_restore:?} continuation {after_suffix:?}\n  prefix {:?}\n  suffix {:?}",
                        case.name,
                        String::from_utf8_lossy(&case.prefix),
                        String::from_utf8_lossy(&case.suffix)
                    );
                }
                (
                    json!(restore.bytes),
                    None,
                    restore.main_screen_missing,
                    restore.approximations.clone(),
                    Some(exact),
                )
            }
        };
        let hidden_alternate = false; // Getter patch restores the latent cursor slot.
        let strict = gap.is_none() && !main_missing && approximations.is_empty();
        if strict && rust_exact == Some(false) {
            strict_failures.push(case.name.clone());
        }
        if case.kind == "named" {
            let status = match (gap, main_missing, rust_exact) {
                (Some(gap), _, _) => format!("DEFER ({gap})"),
                (None, true, _) => "GAP (hidden primary screen)".into(),
                (None, false, Some(true)) => "PASS".into(),
                _ => "FAIL".into(),
            };
            println!("{status} {}", case.name);
        }
        results.push(json!({
            "name": case.name, "kind": case.kind, "prefix": case.prefix, "suffix": case.suffix,
            "resized": case.resized.map(|size| json!({"columns":size.columns(),"rows":size.rows()})),
            "restore": restore_bytes, "gap": gap, "mainScreenMissing": main_missing,
            "approximations": approximations, "hiddenAlternate": hidden_alternate, "rustExact": rust_exact, "expected": expected,
        }));
    }
    let count = |f: &dyn Fn(&Value) -> bool| {
        results
            .iter()
            .filter(|r| r["kind"] == "random" && f(r))
            .count()
    };
    println!(
        "random 600: rust-exact {} | deferred {} | hidden primary {} | hidden alternate {} | approximate {}",
        count(&|r| r["rustExact"] == true),
        count(&|r| !r["gap"].is_null()),
        count(&|r| r["mainScreenMissing"] == true),
        count(&|r| r["hiddenAlternate"] == true && r["mainScreenMissing"] == false),
        count(&|r| r["approximations"]
            .as_array()
            .is_some_and(|a| !a.is_empty())),
    );
    // Single-token divergence matrix: which sequences xterm.js and alacritty
    // interpret differently from a few representative starting states.
    let bases: [(&str, &[u8]); 6] = [
        ("empty", b""),
        ("full-line", b"12345678901234567890123456789012"),
        (
            "wide-row",
            "\x1b[3;1H\u{4e2d}\u{6587}\u{4e2d}\x1b[3;2H".as_bytes(),
        ),
        ("region-origin", b"\x1b[2;5r\x1b[?6h\x1b[2;3Hxy"),
        ("insert-mode", b"ABCDEF\r\x1b[4h"),
        ("line-drawing", b"\x1b(0\x1b)B"),
    ];
    let mut matrix = Vec::new();
    for (base, prefix) in bases {
        for token in TOKENS {
            let mut term = TerminalProbe::new(size, 128);
            term.feed(prefix);
            term.feed(token.as_bytes());
            term.finish_input();
            let bytes = [prefix, token.as_bytes()].concat();
            matrix.push(
                json!({"base": base, "token": token, "bytes": bytes, "expected": term.screen()}),
            );
        }
    }
    // Restore size and cost for a 120x40 screen (worst case: every cell has its own
    // colours). The serializer is not optimised (CUP + full SGR per cell).
    let big = TermSize::new(120, 40)?;
    let mut worst_term = TerminalProbe::new(big, 10_000);
    let mut plain_term = TerminalProbe::new(big, 10_000);
    for row in 0..40u32 {
        for col in 0..120u32 {
            let colour = (row * 120 + col) % 256;
            let glyph = char::from(b'!' + (col % 90) as u8);
            worst_term.feed(format!("\x1b[38;5;{colour};48;5;{}m{glyph}", 255 - colour).as_bytes());
        }
        plain_term.feed(&[b'x'; 120]);
    }
    let started = std::time::Instant::now();
    let worst = worst_term
        .restore_sequence()
        .map(|r| r.bytes.len())
        .unwrap_or(0);
    let elapsed = started.elapsed();
    let plain = plain_term
        .restore_sequence()
        .map(|r| r.bytes.len())
        .unwrap_or(0);
    println!("restore size 120x40: worst {worst} bytes in {elapsed:?}; plain text {plain} bytes");
    std::fs::create_dir_all("target/probes")?;
    std::fs::write(
        "target/probes/token-matrix.json",
        serde_json::to_vec_pretty(&json!({"columns": 32, "rows": 8, "entries": matrix}))?,
    )?;
    std::fs::write(
        "target/probes/snapshot.json",
        serde_json::to_vec_pretty(
            &json!({"schemaVersion": 2, "columns": 32, "rows": 8, "cases": results}),
        )?,
    )?;
    ensure!(
        strict_failures.is_empty(),
        "restore not exact for: {strict_failures:?}"
    );
    Ok(())
}
