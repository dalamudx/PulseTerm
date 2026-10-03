import headless from "@xterm/headless";
import { expect, it } from "vitest";
import queries from "../../../../tests/fixtures/terminal/queries.json";
import { suppressQueryReplies } from "./query-policy";

const { Terminal } = headless;
const write = (terminal: InstanceType<typeof Terminal>, bytes: string) =>
  new Promise<void>((resolve) => terminal.write(bytes, resolve));

it("suppresses parser replies but keeps onData subscribed", async () => {
  const terminal = new Terminal({ cols: 32, rows: 8, allowProposedApi: true });
  const replies: string[] = [];
  terminal.onData((data) => replies.push(data));
  await write(terminal, "\x1b[6n");
  expect(replies).toEqual(["\x1b[1;1R"]);
  replies.length = 0;
  const dispose = suppressQueryReplies(terminal);
  await write(
    terminal,
    "\x1b[c\x1b[>c\x1b[6n\x1b[?6n\x1b[?25$p\x1b[18t\x1bP$qm\x1b\\\x1b]10;?\x07",
  );
  expect(replies).toEqual([]);
  dispose();
  await write(terminal, "\x1b[6n");
  expect(replies).toEqual(["\x1b[1;1R"]);
  terminal.dispose();
});

it.each(queries.cases)("suppresses fragmented query $name", async (entry) => {
  const bytes = new TextEncoder().encode(entry.input);
  for (const chunkSize of new Set([1, 2, 3, 7, Math.max(1, bytes.length)])) {
    const terminal = new Terminal({
      cols: queries.columns,
      rows: queries.rows,
      allowProposedApi: true,
    });
    const replies: string[] = [];
    terminal.onData((data) => replies.push(data));
    const dispose = suppressQueryReplies(terminal);
    try {
      for (let offset = 0; offset < bytes.length; offset += chunkSize) {
        await new Promise<void>((resolve) =>
          terminal.write(bytes.subarray(offset, offset + chunkSize), resolve),
        );
      }
      expect(replies, `${entry.name}: ${chunkSize}-byte chunks`).toEqual([]);
    } finally {
      dispose();
      terminal.dispose();
    }
  }
});

it("suppresses public DECRQM without consuming the mode setter", async () => {
  const terminal = new Terminal({ allowProposedApi: true });
  const replies: string[] = [];
  terminal.onData((data) => replies.push(data));
  await write(terminal, "\x1b[4$p");
  expect(replies).toEqual(["\x1b[4;2$y"]);
  replies.length = 0;
  const dispose = suppressQueryReplies(terminal);
  await write(terminal, "\x1b[4h\x1b[4$p");
  expect(replies).toEqual([]);
  dispose();
  await write(terminal, "\x1b[4$p");
  expect(replies).toEqual(["\x1b[4;1$y"]);
  terminal.dispose();
});

it("does not swallow palette changes as if they were queries", async () => {
  const terminal = new Terminal({ allowProposedApi: true });
  const observed: string[] = [];
  // Register the fallback first: policy hooks are tried newest first.
  const hook = terminal.parser.registerOscHandler(4, (data) => {
    observed.push(data);
    return false;
  });
  const dispose = suppressQueryReplies(terminal);
  await write(terminal, "\x1b]4;1;rgb:ff/00/00\x07\x1b]4;1;?\x07");
  expect(observed).toEqual(["1;rgb:ff/00/00"]);
  dispose();
  hook.dispose();
  terminal.dispose();
});
