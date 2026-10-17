import { describe, expect, it } from "vitest";
import {
  MAX_THEME_PAINT_BYTES,
  parseThemePaintData,
  type Rgb8,
  rgbToHex,
  THEME_UI_PROPERTIES,
  ThemePaintError,
} from "./paint-data";

function fixture() {
  return {
    version: 1,
    appearance: "dark",
    neutral: "slate",
    accent: "cyan",
    terminal: {
      foreground: [255, 255, 255],
      background: [0, 0, 0],
      ansi: Array.from({ length: 16 }, (_, i) => [i, 128, 255]),
    },
    overrides: [{ slot: "bgBase", color: [1, 2, 3] }],
    lowForegroundContrast: false,
  };
}
function encoded(change: (v: Record<string, unknown>) => void) {
  const v: Record<string, unknown> = fixture();
  change(v);
  return JSON.stringify(v);
}
describe("fixed theme paint v1 data, not application authority", () => {
  it("preserves all colors and deeply freezes data", () => {
    const d = parseThemePaintData(JSON.stringify(fixture()));
    expect(d.terminal.ansi).toHaveLength(16);
    expect(d.terminal.ansi[13]).toEqual([13, 128, 255]);
    const first = d.overrides[0],
      firstAnsi = d.terminal.ansi[0];
    if (!first || !firstAnsi) throw new Error("missing fixture color");
    expect(rgbToHex(first.color)).toBe("#010203");
    for (const value of [
      d,
      d.terminal,
      d.terminal.ansi,
      firstAnsi,
      d.overrides,
      first,
      first.color,
      THEME_UI_PROPERTIES,
    ])
      expect(Object.isFrozen(value)).toBe(true);
    expect(Reflect.set(d.terminal.ansi, "length", 0)).toBe(false);
  });
  it("does not infer missing optional colors", () => {
    const d = parseThemePaintData(JSON.stringify(fixture()));
    for (const key of [
      "cursor",
      "cursorText",
      "selectionBackground",
      "selectionForeground",
    ] as const)
      expect(d.terminal[key]).toBeUndefined();
  });
  it("preserves four optional channels", () => {
    const v = fixture();
    Object.assign(v.terminal, {
      cursor: [171, 205, 239],
      cursorText: [1, 2, 3],
      selectionBackground: [4, 5, 6],
      selectionForeground: [7, 8, 9],
    });
    const d = parseThemePaintData(JSON.stringify(v));
    expect(d.terminal.cursor).toEqual([171, 205, 239]);
    expect(d.terminal.cursorText).toEqual([1, 2, 3]);
    expect(d.terminal.selectionBackground).toEqual([4, 5, 6]);
    expect(d.terminal.selectionForeground).toEqual([7, 8, 9]);
  });
  it("maps every fixed slot and keeps terminal contrast warning", () => {
    const v = fixture();
    v.overrides = Object.keys(THEME_UI_PROPERTIES).map((slot) => ({
      slot,
      color: [255, 255, 255],
    }));
    v.lowForegroundContrast = true;
    const d = parseThemePaintData(JSON.stringify(v));
    expect(d.overrides).toHaveLength(28);
    expect(d.lowForegroundContrast).toBe(true);
    expect(d.terminal.foreground).toEqual([255, 255, 255]);
  });
  it("rejects unsupported, missing and nonnumeric versions", () => {
    for (const version of [2, 0, null, "1"]) {
      expect(() =>
        parseThemePaintData(
          encoded((v) => {
            v.version = version;
          }),
        ),
      ).toThrow("theme.paint.version");
    }
    expect(() =>
      parseThemePaintData(
        encoded((v) => {
          delete v.version;
        }),
      ),
    ).toThrow("theme.paint.invalid");
  });
  it("rejects unknown and missing fields without echoing input", () => {
    for (const key of [
      "version",
      "appearance",
      "neutral",
      "accent",
      "terminal",
      "overrides",
      "lowForegroundContrast",
    ]) {
      expect(() =>
        parseThemePaintData(
          encoded((v) => {
            delete v[key];
          }),
        ),
      ).toThrow(ThemePaintError);
    }
    for (const key of ["name", "path", "css", "__proto__"]) {
      const input = encoded((v) => {
        Object.defineProperty(v, key, {
          value: "PRIVATE /../../theme",
          enumerable: true,
        });
      });
      expect(() => parseThemePaintData(input)).toThrow("theme.paint.invalid");
    }
    expect(() => parseThemePaintData("{PRIVATE /../../theme")).toThrow(
      "theme.paint.invalid",
    );
  });
  it("rejects duplicate keys, escaped keys and noncanonical numeric/whitespace forms", () => {
    const input = JSON.stringify(fixture());
    for (const wrong of [
      input.replace('"version":1', '"version":2,"version":1'),
      input.replace('"version"', '"\\u0076ersion"'),
      input.replace('"version":1', '"version":1.0'),
      `${input} `,
    ])
      expect(() => parseThemePaintData(wrong)).toThrow("theme.paint.invalid");
  });
  it("enforces byte quota before parse and content checks", () => {
    expect(() =>
      parseThemePaintData("x".repeat(MAX_THEME_PAINT_BYTES + 1)),
    ).toThrow("theme.paint.budget");
    expect(() => parseThemePaintData(`"${"😀".repeat(1100)}"`)).toThrow(
      "theme.paint.budget",
    );
    expect(() => parseThemePaintData(null)).toThrow("theme.paint.invalid");
  });
  it("rejects invalid RGB triples, incomplete ANSI and nested extras", () => {
    for (const color of [
      [-1, 0, 0],
      [256, 0, 0],
      [0.5, 0, 0],
      [0, 0],
      [0, 0, 0, 0],
      ["0", 0, 0],
      null,
    ]) {
      const v = fixture();
      Object.assign(v.terminal, { foreground: color });
      expect(() => parseThemePaintData(JSON.stringify(v))).toThrow(
        "theme.paint.invalid",
      );
    }
    for (const length of [0, 15, 17]) {
      const v = fixture();
      v.terminal.ansi = Array.from({ length }, () => [0, 0, 0]);
      expect(() => parseThemePaintData(JSON.stringify(v))).toThrow(
        "theme.paint.invalid",
      );
    }
    const v = fixture();
    Object.assign(v.terminal, { css: "url(PRIVATE)" });
    expect(() => parseThemePaintData(JSON.stringify(v))).toThrow(
      "theme.paint.invalid",
    );
  });
  it("rejects duplicate/prototype/unknown UI slots and excess count", () => {
    for (const slot of ["__proto__", "constructor", "bgBase;url(x)"]) {
      const v = fixture();
      v.overrides = [{ slot, color: [0, 0, 0] }];
      expect(() => parseThemePaintData(JSON.stringify(v))).toThrow(
        "theme.paint.invalid",
      );
    }
    for (const length of [2, 29]) {
      const v = fixture();
      v.overrides = Array.from({ length }, () => ({
        slot: "bgBase",
        color: [0, 0, 0],
      }));
      expect(() => parseThemePaintData(JSON.stringify(v))).toThrow(
        "theme.paint.invalid",
      );
    }
  });
  it("requires exact appearance, neutral, accent and boolean warning", () => {
    for (const change of [
      { appearance: "system" },
      { neutral: "--font-size" },
      { accent: "url(x)" },
      { accent: "#abcdef00" },
      { accent: "#ABCDEF" },
      { lowForegroundContrast: "false" },
    ])
      expect(() =>
        parseThemePaintData(JSON.stringify({ ...fixture(), ...change })),
      ).toThrow("theme.paint.invalid");
    const v = {
      ...fixture(),
      appearance: "light",
      neutral: "sand",
      accent: "#abcdef",
    };
    expect(parseThemePaintData(JSON.stringify(v)).accent).toBe("#abcdef");
  });
  it("revalidates at hex formatting boundary", () => {
    expect(rgbToHex([0, 128, 255])).toBe("#0080ff");
    expect(() => rgbToHex([0, "url(x)", 0] as unknown as Rgb8)).toThrow(
      "theme.paint.invalid",
    );
  });
});
