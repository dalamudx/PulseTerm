import { Terminal } from "@xterm/headless";
import { describe, expect, it } from "vitest";
import {
  MAX_QUERY_PALETTE_BYTES,
  parseQueryPaletteData,
  queryPaletteTheme,
} from "./query-palette";

const palette = {
  ansi: Array.from({ length: 16 }, (_, i) => [i, i + 16, i + 32]),
  foreground: [1, 2, 3],
  background: [4, 5, 6],
  cursor: [7, 8, 9],
};
const envelope = () => ({ version: 1, palette: structuredClone(palette) });
const data = () => parseQueryPaletteData(JSON.stringify(envelope()));
const write = (t: Terminal, s: string) =>
  new Promise<void>((resolve) => t.write(s, resolve));
describe("captured query palette", () => {
  it("maps all 19 fixed base colors without selection guesses or OSC injection", () => {
    const value = data(),
      theme = queryPaletteTheme(value);
    expect(Object.keys(theme)).toHaveLength(19);
    expect(theme).toMatchObject({
      black: "#001020",
      magenta: "#051525",
      brightMagenta: "#0d1d2d",
      brightWhite: "#0f1f2f",
      foreground: "#010203",
      background: "#040506",
      cursor: "#070809",
    });
    expect(Object.isFrozen(value)).toBe(true);
    expect(Object.isFrozen(value.ansi[0])).toBe(true);
    expect(Object.isFrozen(theme)).toBe(true);
  });
  it.each([
    (v: ReturnType<typeof envelope>) => ({ ...v, version: 2 }),
    (v: ReturnType<typeof envelope>) => ({ ...v, path: "private-fixture" }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, selectionBackground: [1, 2, 3] },
    }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, ansi: v.palette.ansi.slice(1) },
    }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, foreground: [256, 2, 3] },
    }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, background: [-1, 2, 3] },
    }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, cursor: [1.5, 2, 3] },
    }),
    (v: ReturnType<typeof envelope>) => ({
      ...v,
      palette: { ...v.palette, cursor: "url(private-fixture)" },
    }),
    (v: ReturnType<typeof envelope>) => ({ ...v, palette: null }),
  ])("rejects invalid bounded metadata without echoing content", (mutate) => {
    expect(() =>
      parseQueryPaletteData(JSON.stringify(mutate(envelope()))),
    ).toThrow("terminal.palette.invalid");
  });
  it("rejects duplicate keys, noncanonical encoding and excessive size", () => {
    const encoded = JSON.stringify(envelope());
    expect(() =>
      parseQueryPaletteData(
        encoded.replace('"version":1', '"version":1,"version":1'),
      ),
    ).toThrow();
    expect(() => parseQueryPaletteData(` ${encoded}`)).toThrow();
    expect(() =>
      parseQueryPaletteData(" ".repeat(MAX_QUERY_PALETTE_BYTES + 1)),
    ).toThrow();
  });
  it("revalidates even typed objects at the xterm string boundary", () => {
    expect(() =>
      queryPaletteTheme({ ...data(), foreground: [NaN, 2, 3] }),
    ).toThrow("terminal.palette.invalid");
  });
  it("headless cannot witness displayed palette resets or color-query equality", async () => {
    const terminal = new Terminal({
      cols: 32,
      rows: 8,
      allowProposedApi: true,
      theme: queryPaletteTheme(data()),
    });
    const replies: string[] = [];
    terminal.onData((s) =>
      replies.push(s.endsWith("\x1b\\") ? `${s.slice(0, -2)}\x07` : s),
    );
    try {
      await write(
        terminal,
        "\x1b]4;1;#abcdef\x07\x1b]10;#102030\x07\x1b]4;1;?\x07\x1b]10;?\x07",
      );
      // The failed equality probe is retained as a capability boundary, not
      // silently accepted as browser colour evidence. Actual reset checks run
      // in the fixed real WebView target with its ThemeService.
      expect(replies).toEqual([]);
    } finally {
      terminal.dispose();
    }
  });
});
