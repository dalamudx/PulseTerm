// Restore fidelity in the real display consumer (xterm.js headless).
// M1: xterm(restore + suffix) == Rust(prefix + suffix)   display matches authority
// M2: xterm(prefix + suffix)  == Rust(prefix + suffix)   baseline parser agreement
// M3: xterm(restore + suffix) == xterm(prefix + suffix)  restore vs. full replay in xterm
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { isDeepStrictEqual } from "node:util";
import { runXterm } from "./xterm-screen";

type Entry = {
  name: string;
  kind: string;
  prefix: number[];
  suffix: number[];
  restore: number[] | null;
  gap: string | null;
  mainScreenMissing: boolean;
  hiddenAlternate: boolean;
  approximations: string[];
  rustExact: boolean | null;
  expected: unknown;
};
const report = JSON.parse(
  readFileSync(
    new URL("../../target/probes/snapshot.json", import.meta.url),
    "utf8",
  ),
) as {
  schemaVersion: number;
  columns: number;
  rows: number;
  cases: Entry[];
};
assert.equal(report.schemaVersion, 2);
const bytes = (values: number[]) => Uint8Array.from(values);
const firstDiff = (
  a: { cells: unknown[][]; cursor: unknown },
  b: { cells: unknown[][]; cursor: unknown },
) => {
  for (let row = 0; row < a.cells.length; row++)
    for (let col = 0; col < (a.cells[row]?.length ?? 0); col++)
      if (!isDeepStrictEqual(a.cells[row]?.[col], b.cells[row]?.[col]))
        return {
          row,
          col,
          xterm: a.cells[row]?.[col],
          rust: b.cells[row]?.[col],
        };
  return isDeepStrictEqual(a.cursor, b.cursor)
    ? null
    : { xtermCursor: a.cursor, rustCursor: b.cursor };
};
const results = [];
const tally = { strict: 0, m1: 0, m2: 0, m3: 0, m1GivenM2: 0, m2Count: 0 };
const namedFailures: string[] = [];
for (const entry of report.cases) {
  if (entry.restore === null) {
    results.push({
      name: entry.name,
      kind: entry.kind,
      status: `deferred:${entry.gap}`,
    });
    continue;
  }
  const direct = await runXterm(
    [bytes(entry.prefix), bytes(entry.suffix)],
    report.columns,
    report.rows,
  );
  const restored = await runXterm(
    [bytes(entry.restore), bytes(entry.suffix)],
    report.columns,
    report.rows,
  );
  const expected = entry.expected as { cells: unknown[][]; cursor: unknown };
  const m1 = isDeepStrictEqual(restored.screen, expected);
  const m2 = isDeepStrictEqual(direct.screen, expected);
  const m3 =
    isDeepStrictEqual(restored.screen, direct.screen) &&
    isDeepStrictEqual(restored.modes, direct.modes);
  const strict =
    !entry.mainScreenMissing &&
    !entry.hiddenAlternate &&
    entry.approximations.length === 0;
  if (strict) {
    tally.strict++;
    tally.m1 += Number(m1);
    tally.m2 += Number(m2);
    tally.m3 += Number(m3);
    if (m2) {
      tally.m2Count++;
      tally.m1GivenM2 += Number(m1);
    }
  }
  const status = !strict
    ? entry.mainScreenMissing
      ? "gap:hidden-primary"
      : entry.hiddenAlternate
        ? "gap:hidden-alternate"
        : "approximate"
    : m1
      ? "pass"
      : m2
        ? "restore-fail"
        : "parser-differs";
  if (entry.kind === "named") {
    console.log(
      `${status.toUpperCase()} ${entry.name}${m3 ? "" : " (xterm restore != xterm replay)"}`,
    );
    if (strict && m2 && !m1) namedFailures.push(entry.name);
  }
  results.push({
    name: entry.name,
    kind: entry.kind,
    status,
    m1,
    m2,
    m3,
    restoreDiff: m1
      ? null
      : firstDiff(restored.screen as never, expected as never),
    directDiff: m2
      ? null
      : firstDiff(direct.screen as never, expected as never),
    modesDiff: isDeepStrictEqual(restored.modes, direct.modes)
      ? null
      : { restored: restored.modes, direct: direct.modes },
  });
}
console.log(
  `strict cases ${tally.strict}: M1 ${tally.m1}, M2 ${tally.m2}, M3 ${tally.m3}, M1|M2 ${tally.m1GivenM2}/${tally.m2Count}`,
);
writeFileSync(
  new URL("../../target/probes/snapshot-xterm.json", import.meta.url),
  JSON.stringify({ tally, results }, null, 2),
);
assert.deepEqual(
  namedFailures,
  [],
  "named restore cases fail in xterm although xterm parses the original stream identically",
);
