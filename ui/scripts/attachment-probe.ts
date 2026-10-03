import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import headless from "@xterm/headless";
import { suppressQueryReplies } from "../src/features/terminal/query-policy";
import { SharedUnicodeProvider } from "../src/features/terminal/unicode-provider";
import { Utf8Stream } from "../src/features/terminal/utf8-stream";
import { AttachmentConsumer } from "../src/ipc/attachment-consumer";
import { observeCell, screen } from "./xterm-screen";

interface Case {
  name: string;
  frames: number[][];
  retainedRows: number;
  totalRows: number;
  history: unknown;
  expected: unknown;
}
const report = JSON.parse(
  readFileSync(
    new URL("../../target/probes/attachment.json", import.meta.url),
    "utf8",
  ),
) as { columns: number; rows: number; cases: Case[] };
for (const entry of report.cases) {
  const terminal = new headless.Terminal({
    cols: report.columns,
    rows: report.rows,
    scrollback: 128,
    allowProposedApi: true,
  });
  const provider = new SharedUnicodeProvider();
  terminal.unicode.register(provider);
  terminal.unicode.activeVersion = provider.version;
  const suppress = suppressQueryReplies(terminal);
  const replies: string[] = [];
  terminal.onData((s) => replies.push(s));
  let decoder = new Utf8Stream();
  let rendered = 0;
  let readyRendered = false;
  let acked = 0;
  const generation = 0x100000000000007n;
  const consumer = new AttachmentConsumer(
    generation,
    {
      async reset(boundary, signal) {
        assert.ok(!signal.aborted);
        assert.equal(boundary.columns, report.columns);
        terminal.reset();
        decoder = new Utf8Stream();
      },
      async write(bytes, signal) {
        for (let offset = 0; offset < bytes.length; offset += 31) {
          assert.ok(!signal.aborted);
          const text = decoder.feed(bytes.subarray(offset, offset + 31));
          if (text.length > 0)
            await new Promise<void>((done) => terminal.write(text, done));
        }
        rendered++;
      },
    },
    async (_, seq) => {
      assert.equal(seq, acked);
      const part = entry.frames[seq]?.[7];
      if (part === 4) {
        assert.ok(rendered >= 2);
        readyRendered = true;
      }
      if (part === 5) assert.ok(readyRendered);
      acked++;
    },
    2,
    2048,
  );
  try {
    for (const frame of entry.frames) {
      const receipt = await consumer.enqueue(Uint8Array.from(frame));
      if (receipt?.event.kind === "ready") {
        const buffer = terminal.buffer.normal;
        assert.equal(buffer.baseY, entry.retainedRows, entry.name);
        const history = Array.from({ length: buffer.baseY }, (_, row) => {
          const line = buffer.getLine(row);
          assert.ok(line);
          return {
            wrapped: line.isWrapped,
            cells: Array.from({ length: terminal.cols }, (_, column) => {
              const cell = line.getCell(column);
              assert.ok(cell);
              return observeCell(cell);
            }),
          };
        });
        assert.deepEqual(
          history,
          entry.history,
          `${entry.name}: history colours/soft wraps`,
        );
      }
    }
    assert.deepEqual(
      screen(terminal),
      entry.expected,
      `${entry.name}: final screen`,
    );
    assert.deepEqual(replies, [], `${entry.name}: no replay query leakage`);
    assert.equal(acked, entry.frames.length);
    assert.equal(consumer.queuedBytes, 0);
    console.log(
      `PASS ${entry.name}: ${entry.frames.length} ordered binary frames, ${entry.retainedRows}/${entry.totalRows} history rows, callback-gated ACK`,
    );
  } finally {
    consumer.cancel();
    suppress();
    terminal.dispose();
  }
}
