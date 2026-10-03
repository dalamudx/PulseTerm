// Diagnostic comparison only. Unknown/mismatching emoji are NEVER S2 passes.
import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import graphemes from "@xterm/addon-unicode-graphemes";
import unicode11 from "@xterm/addon-unicode11";
import headless from "@xterm/headless";
import { SharedUnicodeProvider } from "../src/features/terminal/unicode-provider";
import { Utf8Stream } from "../src/features/terminal/utf8-stream";

const { Terminal } = headless;
const { Unicode11Addon } = unicode11;
const { UnicodeGraphemesAddon } = graphemes;
function object(value: unknown): Record<string, unknown> {
  assert.ok(
    value !== null && typeof value === "object" && !Array.isArray(value),
  );
  return value as Record<string, unknown>;
}
const report = object(
  JSON.parse(
    readFileSync(
      new URL("../../target/probes/terminal.json", import.meta.url),
      "utf8",
    ),
  ) as unknown,
);
assert.equal(report.schema_version, 1);
assert.ok(Array.isArray(report.unicode) && report.unicode.length > 0);
const cols = report.columns;
const rows = report.rows;
assert.ok(typeof cols === "number" && typeof rows === "number");
const providers = ["6", "11", "15", "15-graphemes", "shared-17"] as const;
const results = [];
let strictFailures = 0;
for (const provider of providers) {
  for (const raw of report.unicode) {
    const entry = object(raw);
    const input = entry.input;
    assert.equal(typeof input, "string");
    const expected = object(entry.expected);
    assert.ok(Array.isArray(expected.cells));
    const project = (cells: unknown) => {
      assert.ok(Array.isArray(cells));
      return cells.map((row: unknown) => {
        assert.ok(Array.isArray(row));
        return row.map((rawCell: unknown) => {
          const cell = object(rawCell);
          return { text: cell.text, width: cell.width };
        });
      });
    };
    const expectedCells = project(expected.cells);
    const chunks = new Set([
      1,
      2,
      3,
      7,
      new TextEncoder().encode(input as string).length,
    ]);
    let matched = true;
    let partitionStable = true;
    let reference: unknown;
    let actualCursor: unknown;
    let firstDifference: unknown;
    for (const chunkSize of chunks) {
      const terminal = new Terminal({ cols, rows, allowProposedApi: true });
      try {
        if (provider === "11") terminal.loadAddon(new Unicode11Addon());
        if (provider.startsWith("15"))
          terminal.loadAddon(new UnicodeGraphemesAddon());
        const shared = new SharedUnicodeProvider();
        if (provider === "shared-17") terminal.unicode.register(shared);
        const activeVersion =
          provider === "shared-17" ? shared.version : provider;
        assert.ok(terminal.unicode.versions.includes(activeVersion));
        terminal.unicode.activeVersion = activeVersion;
        const bytes = new TextEncoder().encode(input as string);
        const decoder = new Utf8Stream();
        for (let offset = 0; offset < bytes.length; offset += chunkSize) {
          const decoded = decoder.feed(
            bytes.subarray(offset, offset + chunkSize),
          );
          // Hypothesis: don't invoke xterm for decoder-only (zero scalar) chunks.
          if (decoded.length > 0)
            await new Promise<void>((resolve) =>
              terminal.write(decoded, resolve),
            );
        }
        const trailing = decoder.finish();
        if (trailing.length > 0)
          await new Promise<void>((resolve) =>
            terminal.write(trailing, resolve),
          );
        const buffer = terminal.buffer.active;
        const cells: { text: string; width: number }[][] = Array.from(
          { length: rows },
          (_, row) =>
            Array.from({ length: cols }, (_, col) => {
              const cell = buffer.getLine(buffer.baseY + row)?.getCell(col);
              assert.ok(cell);
              return {
                text: cell.getWidth() === 0 ? "" : cell.getChars() || " ",
                width: cell.getWidth(),
              };
            }),
        );
        actualCursor = [buffer.cursorX, buffer.cursorY];
        const actual: { cells: typeof cells; cursor: unknown } = {
          cells,
          cursor: actualCursor,
        };
        if (reference === undefined) reference = actual;
        else {
          try {
            assert.deepEqual(actual, reference);
          } catch {
            partitionStable = false;
          }
        }
        try {
          assert.deepEqual(cells, expectedCells);
          assert.deepEqual(actualCursor, expected.cursor);
        } catch {
          matched = false;
          if (firstDifference === undefined)
            firstDifference = {
              actualRow: cells[0],
              expectedRow: expectedCells[0],
            };
        }
      } finally {
        terminal.dispose();
      }
    }
    // Shared Rust17 scalar widths are the tested contract. Other providers are diagnostics.
    if (provider === "shared-17" && (!matched || !partitionStable))
      strictFailures++;
    const result = {
      provider,
      name: entry.name,
      matched,
      partitionStable,
      actualCursor,
      expectedCursor: expected.cursor,
      firstDifference,
    };
    results.push(result);
    console.log(
      `${matched ? "MATCH" : "DIFF"} ${provider} ${String(entry.name)} cursor=${JSON.stringify(actualCursor)} rust=${JSON.stringify(expected.cursor)}`,
    );
  }
}
writeFileSync(
  new URL("../../target/probes/unicode.json", import.meta.url),
  JSON.stringify(
    {
      schemaVersion: 1,
      acceptancePassed: false,
      backend: "alacritty_terminal 0.26.0 / unicode-width 0.2.2 (Unicode 17)",
      results,
      limitations: [
        "Representative corpus, not full Unicode conformance",
        "Unicode17 backend differs from published xterm11/15 providers",
        "Grapheme addon is experimental",
        "No live font/shaping/IME proof",
        "Diagnostic comparisons are not S2 acceptance",
      ],
    },
    null,
    2,
  ),
);
assert.equal(strictFailures, 0, "Baseline/partition regressions detected");
