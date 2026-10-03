//! Deterministic frontend widths, generated from the same locked table as Term.
use anyhow::{Result, ensure};
use pulse_term::{UNICODE_VERSION, scalar_width};
pub fn run(check: bool) -> Result<()> {
    let mut ranges = Vec::<[u32; 3]>::new();
    let mut hash = 2166136261u32;
    for cp in 0..=0x10ffff {
        let width = scalar_width(cp);
        hash = (hash ^ u32::from(width)).wrapping_mul(16777619);
        if width == 1 {
            continue;
        }
        if let Some(last) = ranges.last_mut()
            && last[1] + 1 == cp
            && last[2] == u32::from(width)
        {
            last[1] = cp;
        } else {
            ranges.push([cp, cp, u32::from(width)]);
        }
    }
    let path = "ui/src/features/terminal/gen/widths.ts";
    let body = format!(
        "// Generated from locked unicode-width; do not edit. MIT OR Apache-2.0.\nexport const unicodeVersion = \"{}.{}.{}\";\nexport const widthHash = {hash};\nexport const widthRanges: readonly (readonly [number, number, number])[] = {};\n",
        UNICODE_VERSION.0,
        UNICODE_VERSION.1,
        UNICODE_VERSION.2,
        serde_json::to_string(&ranges)?
    );
    if check {
        ensure!(
            std::fs::read_to_string(path)? == body,
            "Unicode table drift; run mise run unicode-table"
        );
    } else {
        std::fs::create_dir_all("ui/src/features/terminal/gen")?;
        std::fs::write(path, body)?;
    }
    Ok(())
}
