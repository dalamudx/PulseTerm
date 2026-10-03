import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import unicode from "@xterm/addon-unicode11";
import type { IBufferCell, Terminal as TerminalType } from "@xterm/headless";
import headless from "@xterm/headless";
import { suppressQueryReplies } from "../src/features/terminal/query-policy";

const { Terminal } = headless;
const { Unicode11Addon } = unicode;

function object(value: unknown): Record<string, unknown> {
  assert.ok(
    value !== null && typeof value === "object" && !Array.isArray(value),
  );
  return value as Record<string, unknown>;
}
function text(value: unknown): string {
  assert.equal(typeof value, "string");
  return value as string;
}
function integer(value: unknown): number {
  assert.ok(
    typeof value === "number" && Number.isSafeInteger(value) && value > 0,
  );
  return value;
}
function color(cell: IBufferCell, foreground: boolean): string {
  if (foreground ? cell.isFgDefault() : cell.isBgDefault()) return "default";
  const value = foreground ? cell.getFgColor() : cell.getBgColor();
  if (foreground ? cell.isFgRGB() : cell.isBgRGB())
    return `rgb:${(value >>> 16) & 255},${(value >>> 8) & 255},${value & 255}`;
  return `idx:${value}`;
}
function screen(terminal: TerminalType) {
  const buffer = terminal.buffer.active;
  const cells = Array.from({ length: terminal.rows }, (_, row) => {
    const line = buffer.getLine(buffer.baseY + row);
    assert.ok(line);
    return Array.from({ length: terminal.cols }, (_, col) => {
      const cell = line.getCell(col);
      assert.ok(cell);
      return {
        text: cell.getWidth() === 0 ? "" : cell.getChars() || " ",
        width: cell.getWidth(),
        foreground: color(cell, true),
        background: color(cell, false),
        bold: cell.isBold() !== 0,
        italic: cell.isItalic() !== 0,
        underline: cell.isUnderline() !== 0,
        inverse: cell.isInverse() !== 0,
      };
    });
  });
  return {
    cells,
    cursor: [buffer.cursorX, buffer.cursorY],
    alternate: buffer.type === "alternate",
  };
}
async function parse(input: string, cols: number, rows: number) {
  const terminal = new Terminal({
    cols,
    rows,
    scrollback: 128,
    allowProposedApi: true,
  });
  const addon = new Unicode11Addon();
  terminal.loadAddon(addon);
  terminal.unicode.activeVersion = "11";
  const dispose = suppressQueryReplies(terminal);
  const replies: string[] = [];
  terminal.onData((data) => replies.push(data));
  const bytes = new TextEncoder().encode(input);
  for (let offset = 0; offset < bytes.length; offset += 3) {
    await new Promise<void>((resolve) =>
      terminal.write(bytes.subarray(offset, offset + 3), resolve),
    );
  }
  const result = screen(terminal);
  dispose();
  terminal.dispose();
  return { screen: result, replies };
}

const path = new URL("../../target/probes/terminal.json", import.meta.url);
const report = object(JSON.parse(readFileSync(path, "utf8")) as unknown);
assert.equal(report.schema_version, 1);
assert.ok(Array.isArray(report.cases) && report.cases.length > 0);
const results: {
  name: string;
  passed: boolean;
  exploratory: boolean;
  error?: string;
}[] = [];
for (const raw of report.cases) {
  const entry = object(raw);
  const name = text(entry.name);
  const exploratory = entry.exploratory === true;
  try {
    assert.equal(entry.rust_replay_matches, true);
    const direct = await parse(
      text(entry.input),
      integer(report.columns),
      integer(report.rows),
    );
    const restored = await parse(
      text(entry.replay),
      integer(report.columns),
      integer(report.rows),
    );
    assert.deepEqual(direct.screen, entry.expected);
    assert.deepEqual(restored.screen, entry.expected);
    assert.deepEqual(direct.replies, []);
    assert.deepEqual(restored.replies, []);
    results.push({ name, passed: true, exploratory });
  } catch (error) {
    results.push({
      name,
      passed: false,
      exploratory,
      error: error instanceof Error ? error.message : String(error),
    });
  }
}
writeFileSync(
  new URL("../../target/probes/xterm.json", import.meta.url),
  JSON.stringify(
    { results, memory: report.memory, limitations: report.limitations },
    null,
    2,
  ),
);
for (const result of results)
  console.log(
    `${result.passed ? "PASS" : result.exploratory ? "OPEN" : "FAIL"} ${result.name}`,
  );
console.log(`Memory lower bound: ${JSON.stringify(report.memory)}`);
const failures = results.filter(
  (result) => !result.passed && !result.exploratory,
);
if (failures.length > 0) {
  console.error(
    failures
      .map(({ name, error }) => `${name}: ${error?.slice(0, 600)}`)
      .join("\n"),
  );
  process.exitCode = 1;
}
