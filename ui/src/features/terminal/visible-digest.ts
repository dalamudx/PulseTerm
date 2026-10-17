// Canonical visible projection shared by product and validation renderers.
// No authentication, hidden state or history parity is implied by this checksum.
import type { IBufferCell, Terminal } from "@xterm/xterm";
import { screenDigest } from "./screen-digest";

function color(cell: IBufferCell, foreground: boolean): string {
  if (foreground ? cell.isFgDefault() : cell.isBgDefault()) return "default";
  const value = foreground ? cell.getFgColor() : cell.getBgColor();
  return (foreground ? cell.isFgRGB() : cell.isBgRGB())
    ? `rgb:${(value >>> 16) & 255},${(value >>> 8) & 255},${value & 255}`
    : `idx:${value}`;
}
export function visibleTerminalDigest(
  terminal: Pick<Terminal, "buffer" | "cols" | "rows">,
): string {
  const { cols, rows } = terminal;
  if (
    !Number.isInteger(cols) ||
    cols < 1 ||
    cols > 512 ||
    !Number.isInteger(rows) ||
    rows < 1 ||
    rows > 256
  )
    throw new Error("terminal.geometry_invalid");
  const buffer = terminal.buffer.active;
  return screenDigest({
    cells: Array.from({ length: rows }, (_, row) =>
      Array.from({ length: cols }, (_, col) => {
        const cell = buffer.getLine(buffer.baseY + row)?.getCell(col);
        if (!cell) throw new Error("terminal.cell_missing");
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
      }),
    ),
    cursor: [Math.min(buffer.cursorX, cols - 1), buffer.cursorY],
    alternate: buffer.type === "alternate",
  });
}
