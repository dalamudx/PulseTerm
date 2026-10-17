import { Terminal } from "@xterm/headless";
import { expect, it } from "vitest";
import { projectedRowText } from "./dom-projection";

it("matches the real xterm row after PTY-style backspace-space-backspace line erasure", async () => {
  const terminal = new Terminal({ cols: 32, rows: 8, allowProposedApi: true });
  try {
    await new Promise<void>((resolve) =>
      terminal.write("keep ERASE" + "\b \b".repeat(5), resolve),
    );
    const written =
      terminal.buffer.active.getLine(0)?.translateToString(true) ?? "";
    expect(written).toBe("keep      ");
    // DOM rows represent padding with NBSP and may include a cursor blank.
    expect(projectedRowText("keep" + "\u00a0".repeat(7))).toBe(
      projectedRowText(written),
    );
    expect(projectedRowText("kee" + "\u00a0".repeat(7))).not.toBe(
      projectedRowText(written),
    );
  } finally {
    terminal.dispose();
  }
});
it.each(["\u3000", "\t", "\n", "\u2002", "\u007f"])(
  "does not discard visible-width or control suffix %j",
  (suffix) => {
    expect(projectedRowText("value" + suffix)).not.toBe(
      projectedRowText("value"),
    );
  },
);
it("retains missing glyphs and internal space counts", () => {
  expect(projectedRowText("甲  乙 ")).not.toBe(projectedRowText("甲 乙"));
  expect(projectedRowText("甲 ")).not.toBe(projectedRowText("甲乙"));
  expect(projectedRowText("甲\u00a0乙 ")).toBe(projectedRowText("甲 乙"));
});
