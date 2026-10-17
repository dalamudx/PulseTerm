import type { Terminal } from "@xterm/xterm";
import { expect, it } from "vitest";
import { parseQueryPaletteData } from "../terminal/query-palette";
import type { Rgb8 } from "../theme/paint-data";
import { assertRenderedPalette, verifyPaletteResets } from "./terminal-palette";

const base = parseQueryPaletteData(
  JSON.stringify({
    version: 1,
    palette: {
      ansi: Array.from({ length: 16 }, (_, i) => [i, i + 10, i + 20]),
      foreground: [1, 2, 3],
      background: [4, 5, 6],
      cursor: [7, 8, 9],
    },
  }),
);
function fixture() {
  const c = (rgb: Rgb8) => ({
    rgba: ((rgb[0] << 24) | (rgb[1] << 16) | (rgb[2] << 8) | 255) >>> 0,
  });
  const colors = {
    ansi: Array.from({ length: 256 }, (_, i) => {
      const rgb = base.ansi[i % 16];
      if (!rgb) throw new Error("fixture missing RGB");
      return c(rgb);
    }),
    foreground: c(base.foreground),
    background: c(base.background),
    cursor: c(base.cursor),
  };
  return { _core: { _themeService: { colors } } };
}
it("compares every opaque RGB8 slot in the actual browser shape", () => {
  expect(() => assertRenderedPalette(fixture(), base)).not.toThrow();
});
for (let i = 0; i < 19; i++)
  it(`cannot overlook captured colour ${i}`, () => {
    const terminal = fixture(),
      colors = terminal._core._themeService.colors;
    const color =
      i < 16
        ? colors.ansi[i]
        : [colors.foreground, colors.background, colors.cursor][i - 16];
    if (!color) throw new Error("fixture missing colour");
    color.rgba ^= 0x100;
    expect(() => assertRenderedPalette(terminal, base)).toThrow(
      "render_mismatch",
    );
  });
it("rejects alpha and incomplete browser palettes", () => {
  const t = fixture();
  t._core._themeService.colors.cursor.rgba &= 0xffffff00;
  expect(() => assertRenderedPalette(t, base)).toThrow("render_mismatch");
  t._core._themeService.colors.ansi.pop();
  expect(() => assertRenderedPalette(t, base)).toThrow("browser_required");
});
it("headless or missing renderer internals are never colour evidence", () => {
  for (const value of [
    null,
    {},
    { _core: {} },
    { _core: { _themeService: {} } },
  ])
    expect(() => assertRenderedPalette(value, base)).toThrow(
      "browser_required",
    );
});
it("cancelled diagnostics do not write a byte", async () => {
  let writes = 0;
  const t = {
    write: () => {
      writes++;
    },
  } as unknown as Terminal;
  const life = new AbortController();
  life.abort();
  await expect(verifyPaletteResets(t, base, life.signal)).rejects.toThrow(
    "retired",
  );
  expect(writes).toBe(0);
});
it("retirement exits even when a write callback never returns", async () => {
  let writes = 0;
  const t = {
    write: () => {
      writes++;
    },
  } as unknown as Terminal;
  const life = new AbortController();
  const task = verifyPaletteResets(t, base, life.signal);
  expect(writes).toBe(1);
  life.abort();
  await expect(task).rejects.toThrow("retired");
  expect(writes).toBe(1);
});
