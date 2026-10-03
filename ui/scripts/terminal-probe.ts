import assert from "node:assert/strict";
import { readFileSync, writeFileSync } from "node:fs";
import headless from "@xterm/headless";
import { suppressQueryReplies } from "../src/features/terminal/query-policy";
import { SharedUnicodeProvider } from "../src/features/terminal/unicode-provider";
import { Utf8Stream } from "../src/features/terminal/utf8-stream";
import { screen } from "./xterm-screen";

const { Terminal } = headless;

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
async function parse(input: string, cols: number, rows: number, chunkSize = 3) {
  const terminal = new Terminal({
    cols,
    rows,
    scrollback: 128,
    allowProposedApi: true,
  });
  const provider = new SharedUnicodeProvider();
  terminal.unicode.register(provider);
  terminal.unicode.activeVersion = provider.version;
  const dispose = suppressQueryReplies(terminal);
  const replies: string[] = [];
  terminal.onData((data) => replies.push(data));
  const bytes = new TextEncoder().encode(input);
  const decoder = new Utf8Stream();
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    const decoded = decoder.feed(bytes.subarray(offset, offset + chunkSize));
    if (decoded.length > 0)
      await new Promise<void>((resolve) => terminal.write(decoded, resolve));
  }
  const trailing = decoder.finish();
  if (trailing.length > 0)
    await new Promise<void>((resolve) => terminal.write(trailing, resolve));
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
assert.ok(Array.isArray(report.queries) && report.queries.length > 0);
for (const raw of report.queries) {
  const entry = object(raw);
  const name = `query:${text(entry.name)}`;
  try {
    assert.deepEqual(entry.backend_replies, entry.expected_replies);
    const input = text(entry.input);
    for (const chunkSize of new Set([
      1,
      2,
      3,
      7,
      Math.max(1, new TextEncoder().encode(input).length),
    ])) {
      const direct = await parse(
        input,
        integer(report.columns),
        integer(report.rows),
        chunkSize,
      );
      assert.deepEqual(direct.replies, []);
      assert.deepEqual(direct.screen, entry.expected);
    }
    results.push({ name, passed: true, exploratory: false });
  } catch (error) {
    results.push({
      name,
      passed: false,
      exploratory: false,
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
