// Shared xterm.js headless projection used by the parser and restore probes.
import assert from "node:assert/strict";
import type { IBufferCell, Terminal as TerminalType } from "@xterm/headless";
import headless from "@xterm/headless";
import { suppressQueryReplies } from "../src/features/terminal/query-policy";
import { SharedUnicodeProvider } from "../src/features/terminal/unicode-provider";
import { Utf8Stream } from "../src/features/terminal/utf8-stream";

const { Terminal } = headless;

export function color(cell: IBufferCell, foreground: boolean): string {
  if (foreground ? cell.isFgDefault() : cell.isBgDefault()) return "default";
  const value = foreground ? cell.getFgColor() : cell.getBgColor();
  if (foreground ? cell.isFgRGB() : cell.isBgRGB())
    return `rgb:${(value >>> 16) & 255},${(value >>> 8) & 255},${value & 255}`;
  return `idx:${value}`;
}

export function screen(terminal: TerminalType) {
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
    // xterm.js reports a pending wrap as cursorX === cols; alacritty keeps the
    // cursor in the last column with a separate flag. Project both the same way.
    cursor: [Math.min(buffer.cursorX, terminal.cols - 1), buffer.cursorY],
    alternate: buffer.type === "alternate",
  };
}

/** Feed byte segments in order through the production decoding path. */
export async function runXterm(
  segments: Uint8Array[],
  cols: number,
  rows: number,
) {
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
  const decoder = new Utf8Stream();
  for (const segment of segments) {
    for (let offset = 0; offset < segment.length; offset += 4096) {
      const decoded = decoder.feed(segment.subarray(offset, offset + 4096));
      if (decoded.length > 0)
        await new Promise<void>((resolve) => terminal.write(decoded, resolve));
    }
  }
  const trailing = decoder.finish();
  if (trailing.length > 0)
    await new Promise<void>((resolve) => terminal.write(trailing, resolve));
  const result = {
    screen: screen(terminal),
    replies,
    modes: { ...terminal.modes },
  };
  dispose();
  terminal.dispose();
  return result;
}
