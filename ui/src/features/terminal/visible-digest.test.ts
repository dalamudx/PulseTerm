import { Terminal } from "@xterm/headless";
import { expect, it } from "vitest";
import { type DigestScreen, screenDigest } from "./screen-digest";
import { visibleTerminalDigest } from "./visible-digest";

function cell(text = " ", width = 1) {
  return {
    text,
    width,
    foreground: "default",
    background: "default",
    bold: false,
    italic: false,
    underline: false,
    inverse: false,
  };
}
async function check(
  bytes: string,
  expected: DigestScreen,
  columns: number,
  rows = 1,
) {
  const terminal = new Terminal({
    cols: columns,
    rows,
    allowProposedApi: true,
    scrollback: 16,
  });
  try {
    await new Promise<void>((resolve) => terminal.write(bytes, resolve));
    expect(visibleTerminalDigest(terminal)).toBe(screenDigest(expected));
  } finally {
    terminal.dispose();
  }
}
it("reads actual headless default colors, spaces and cursor using the same canonical projection", async () => {
  await check(
    "A",
    { cells: [[cell("A"), cell()]], cursor: [1, 0], alternate: false },
    2,
  );
});
it("keeps RGB, indexed colors and individual attributes distinct without palette substitution", async () => {
  await check(
    "\x1b[1;3;4;7;38;2;1;2;3;48;5;120mA",
    {
      cells: [
        [
          {
            ...cell("A"),
            foreground: "rgb:1,2,3",
            background: "idx:120",
            bold: true,
            italic: true,
            underline: true,
            inverse: true,
          },
          cell(),
        ],
      ],
      cursor: [1, 0],
      alternate: false,
    },
    2,
  );
});
it("normalizes wide continuation cells without dropping the leading glyph", async () => {
  await check(
    "中",
    {
      cells: [[cell("中", 2), cell("", 0), cell(), cell()]],
      cursor: [2, 0],
      alternate: false,
    },
    4,
  );
});
it("reads baseY screen rather than history and records alternate screen identity", async () => {
  await check(
    "I\r\nJ\r\nK",
    {
      cells: [
        [cell("J"), cell(), cell(), cell()],
        [cell("K"), cell(), cell(), cell()],
      ],
      cursor: [1, 1],
      alternate: false,
    },
    4,
    2,
  );
  await check(
    "\x1b[?1049hA",
    { cells: [[cell("A"), cell()]], cursor: [1, 0], alternate: true },
    2,
  );
});
