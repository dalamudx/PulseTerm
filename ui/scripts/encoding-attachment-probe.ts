import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import headless from "@xterm/headless";
import { suppressQueryReplies } from "../src/features/terminal/query-policy";
import { SharedUnicodeProvider } from "../src/features/terminal/unicode-provider";
import { Utf8Stream } from "../src/features/terminal/utf8-stream";
import { AttachmentV2Consumer } from "../src/ipc/attachment-v2";
import { screen } from "./xterm-screen";

interface Case {
  name: string;
  generation: string;
  frames: number[][];
  expected: unknown;
  sourceBytes: string;
  utf8Bytes: string;
  sourceOnlyPrefix: boolean;
  v1RefusalPreservedOldLease: boolean;
  v2AtomicReplacement: boolean;
  orderedEofAcked: boolean;
  peakCreditBytes: number;
}
const report = JSON.parse(
  readFileSync(
    new URL("../../target/probes/encoding-attachment.json", import.meta.url),
    "utf8",
  ),
) as { schemaVersion: number; columns: number; rows: number; cases: Case[] };
assert.equal(report.schemaVersion, 2);
assert.equal(report.cases.length, 2);
for (const entry of report.cases) {
  assert.ok(
    entry.sourceOnlyPrefix &&
      entry.v1RefusalPreservedOldLease &&
      entry.v2AtomicReplacement &&
      entry.orderedEofAcked,
  );
  assert.ok(entry.peakCreditBytes <= 64);
  const terminal = new headless.Terminal({
    cols: report.columns,
    rows: report.rows,
    scrollback: 128,
    allowProposedApi: true,
  });
  const unicode = new SharedUnicodeProvider();
  terminal.unicode.register(unicode);
  terminal.unicode.activeVersion = unicode.version;
  const suppress = suppressQueryReplies(terminal);
  let decoder = new Utf8Stream();
  let ready = false,
    ended = false,
    acked = 0;
  const replies: string[] = [];
  terminal.onData((text) => replies.push(text));
  const write = async (bytes: Uint8Array) => {
    const text = decoder.feed(bytes);
    if (text) await new Promise<void>((done) => terminal.write(text, done));
  };
  const consumer = new AttachmentV2Consumer(
    BigInt(entry.generation),
    {
      async reset(boundary, signal) {
        assert.ok(!signal.aborted);
        assert.notEqual(boundary.sourceBytes, boundary.utf8Bytes);
        terminal.reset();
        decoder = new Utf8Stream();
      },
      async write(bytes, signal) {
        assert.ok(!signal.aborted);
        await write(bytes);
        ready = true;
      },
      async finish(boundary, signal) {
        assert.ok(!signal.aborted);
        assert.equal(boundary.sourceBytes, BigInt(entry.sourceBytes));
        assert.equal(boundary.utf8Bytes, BigInt(entry.utf8Bytes));
        assert.notEqual(boundary.sourceBytes, boundary.utf8Bytes);
        await new Promise<void>((done) =>
          terminal.write(decoder.finish(), done),
        );
        ended = true;
      },
    },
    (_generation, sequence) => {
      assert.equal(sequence, acked);
      const part = entry.frames[sequence]?.[7];
      if (part === 4 || part === 5) assert.ok(ready);
      if (part === 7) assert.ok(ended);
      acked++;
    },
    1,
    256,
  );
  try {
    for (const wire of entry.frames)
      await consumer.enqueue(Uint8Array.from(wire));
    assert.ok(ended);
    assert.equal(acked, entry.frames.length);
    assert.equal(consumer.queuedBytes, 0);
    assert.deepEqual(screen(terminal), entry.expected);
    assert.deepEqual(replies, []);
    console.log(
      `PASS ${entry.name}: actual actor → v2 binary → headless callback ACK, source=${entry.sourceBytes} UTF8=${entry.utf8Bytes}`,
    );
  } finally {
    consumer.cancel();
    suppress();
    terminal.dispose();
  }
}
