// Which single sequences do xterm.js and alacritty interpret differently?

import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import { isDeepStrictEqual } from "node:util";
import {
  type DigestScreen,
  screenDigest,
} from "../src/features/terminal/screen-digest";
import { runXterm } from "./xterm-screen";

type Cell = Record<string, unknown>;
type Screen = DigestScreen;
const report = JSON.parse(
  readFileSync(
    new URL("../../target/probes/token-matrix.json", import.meta.url),
    "utf8",
  ),
) as {
  columns: number;
  rows: number;
  entries: {
    base: string;
    token: string;
    bytes: number[];
    expected: Screen;
    risk: number;
    digest: string;
    repair: number[] | null;
  }[];
};
const differences: Record<string, string[]> = {};
const details = [];
let repairedCount = 0;
let unsupportedCount = 0;
for (const entry of report.entries) {
  const actual = (
    await runXterm([Uint8Array.from(entry.bytes)], report.columns, report.rows)
  ).screen as Screen;
  assert.equal(
    screenDigest(entry.expected),
    entry.digest,
    "cross-language digest fixture",
  );
  if (!isDeepStrictEqual(actual, entry.expected))
    assert.notEqual(
      entry.risk,
      0,
      `unmarked divergence ${entry.base} ${JSON.stringify(entry.token)}`,
    );
  // Compare with and without the underline-on-hyperlink projection difference.
  if (isDeepStrictEqual(actual, entry.expected)) continue;
  if (entry.repair !== null) {
    const repaired = await runXterm(
      [Uint8Array.from(entry.bytes), Uint8Array.from(entry.repair)],
      report.columns,
      report.rows,
    );
    repairedCount++;
    assert.equal(
      isDeepStrictEqual(repaired.screen, entry.expected),
      true,
      `screen-only repair ${entry.base} ${JSON.stringify(entry.token)}`,
    );
  }
  if (entry.repair === null) unsupportedCount++;
  let kind = "cursor";
  outer: for (let row = 0; row < actual.cells.length; row++)
    for (let col = 0; col < (actual.cells[row]?.length ?? 0); col++) {
      const a: Cell = actual.cells[row]?.[col] ?? {};
      const e: Cell = entry.expected.cells[row]?.[col] ?? {};
      if (!isDeepStrictEqual(a, e)) {
        const fields = Object.keys(e).filter(
          (key) => !isDeepStrictEqual(a[key], e[key]),
        );
        kind = `cell(${fields.join(",")})`;
        details.push({
          base: entry.base,
          token: JSON.stringify(entry.token),
          row,
          col,
          xterm: a,
          rust: e,
          xtermCursor: actual.cursor,
          rustCursor: entry.expected.cursor,
        });
        break outer;
      }
    }
  if (kind === "cursor")
    details.push({
      base: entry.base,
      token: JSON.stringify(entry.token),
      xtermCursor: actual.cursor,
      rustCursor: entry.expected.cursor,
    });
  const key = JSON.stringify(entry.token);
  const where = differences[key] ?? [];
  where.push(`${entry.base}:${kind}`);
  differences[key] = where;
}
for (const [token, where] of Object.entries(differences))
  console.log(`${token}  ${where.join("  ")}`);
console.log(
  `${Object.keys(differences).length} tokens differ in at least one base; ${details.length}/${report.entries.length} entries`,
);
console.log(
  `Repair exact ${repairedCount}; explicitly unrepresentable/deferred ${unsupportedCount}; all differences risk-marked.`,
);
writeFileSync(
  new URL("../../target/probes/token-matrix-xterm.json", import.meta.url),
  JSON.stringify(details, null, 2),
);
