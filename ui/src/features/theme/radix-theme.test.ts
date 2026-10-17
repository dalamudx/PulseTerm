import {
  amberDark,
  blueDark,
  cyan,
  cyanDark,
  greenDark,
  redDark,
  slate,
  slateDark,
} from "@radix-ui/colors";
import { expect, it } from "vitest";
import {
  parseThemePaintData,
  rgbToHex,
  THEME_UI_PROPERTIES,
  type ThemePaintData,
} from "./paint-data";
import {
  RADIX_THEME_FAMILIES,
  resolveUiTheme,
  uiContrastRatio,
} from "./radix-theme";

function image(
  appearance = "dark",
  neutral = "slate",
  accent = "cyan",
  overrides: unknown[] = [],
) {
  return parseThemePaintData(
    JSON.stringify({
      version: 1,
      appearance,
      neutral,
      accent,
      terminal: {
        foreground: [255, 255, 255],
        background: [0, 0, 0],
        ansi: Array.from({ length: 16 }, () => [255, 128, 0]),
      },
      overrides,
      lowForegroundContrast: false,
    }),
  );
}
function colors(data: ThemePaintData) {
  return Object.fromEntries(
    data.overrides.map((v) => [v.slot, rgbToHex(v.color)]),
  );
}
it("maps all 28 fixed slots to exact Radix steps, not terminal colors", () => {
  for (const appearance of ["light", "dark"]) {
    const data = image(appearance),
      result = resolveUiTheme(data),
      c = colors(result.paint);
    const n = appearance === "light" ? slate : slateDark,
      a = appearance === "light" ? cyan : cyanDark;
    expect(Object.keys(c)).toEqual(Object.keys(THEME_UI_PROPERTIES));
    expect([
      c.bgBase,
      c.bgSubtle,
      c.bgSurface,
      c.bgHover,
      c.bgActive,
      c.borderSubtle,
      c.border,
      c.borderStrong,
      c.fgMuted,
      c.fg,
    ]).toEqual([
      n.slate1,
      n.slate2,
      n.slate3,
      n.slate4,
      n.slate5,
      n.slate6,
      n.slate7,
      n.slate8,
      n.slate11,
      n.slate12,
    ]);
    expect([
      c.accentBg,
      c.accentBgHover,
      c.accent,
      c.accentHover,
      c.accentFg,
    ]).toEqual([a.cyan3, a.cyan4, a.cyan9, a.cyan10, a.cyan11]);
    expect(result.paint.terminal).toEqual(data.terminal);
    expect(result.paint.lowForegroundContrast).toBe(data.lowForegroundContrast);
    expect(Object.isFrozen(result.paint.overrides)).toBe(true);
    expect(parseThemePaintData(JSON.stringify(result.paint))).toEqual(
      result.paint,
    );
  }
});
it("covers 372 stock light/dark combinations with bounded warnings and exact output roundtrips", () => {
  let count = 0;
  for (const appearance of ["light", "dark"])
    for (const neutral of ["gray", "mauve", "slate", "sage", "olive", "sand"])
      for (const accent of RADIX_THEME_FAMILIES) {
        const result = resolveUiTheme(image(appearance, neutral, accent));
        expect(result.paint.overrides).toHaveLength(28);
        expect(result.warnings.length).toBeLessThanOrEqual(25);
        expect(
          result.warnings.every(
            (v) => Number.isFinite(v.ratio) && v.ratio >= 1 && v.ratio < 4.5,
          ),
        ).toBe(true);
        expect(parseThemePaintData(JSON.stringify(result.paint))).toEqual(
          result.paint,
        );
        count++;
      }
  expect(count).toBe(372);
});
it("fixed status scales follow appearance and never follow accent", () => {
  const c = colors(resolveUiTheme(image("dark", "sand", "purple")).paint);
  expect([c.dangerBg, c.danger, c.dangerFg]).toEqual([
    redDark.red3,
    redDark.red9,
    redDark.red11,
  ]);
  expect([c.warningBg, c.warning, c.warningFg]).toEqual([
    amberDark.amber3,
    amberDark.amber9,
    amberDark.amber11,
  ]);
  expect([c.successBg, c.success, c.successFg]).toEqual([
    greenDark.green3,
    greenDark.green9,
    greenDark.green11,
  ]);
  expect([c.infoBg, c.info, c.infoFg]).toEqual([
    blueDark.blue3,
    blueDark.blue9,
    blueDark.blue11,
  ]);
});
it("overrides are exact and reports are advisory unless explicit apply protection is requested", () => {
  const data = image("light", "slate", "cyan", [
    { slot: "fg", color: [255, 255, 255] },
    { slot: "bgBase", color: [255, 255, 255] },
  ]);
  const encoded = JSON.stringify(data),
    result = resolveUiTheme(data);
  expect(result.warnings).toContainEqual({
    foreground: "fg",
    background: "bgBase",
    ratio: 1,
  });
  expect(colors(result.paint).fg).toBe("#ffffff");
  expect(JSON.stringify(data)).toBe(encoded);
  expect(() => resolveUiTheme(data, { contrast: "reject" })).toThrow(
    "theme.ui.low_contrast",
  );
  expect(resolveUiTheme(image(), { contrast: "reject" }).warnings).toEqual([]);
});
it("rejects the old low-contrast native fixture while preserving advisory data exactly", () => {
  const old = image("dark", "slate", "cyan", [
    { slot: "bgBase", color: [119, 136, 153] },
  ]);
  expect(resolveUiTheme(old).warnings.length).toBeGreaterThan(0);
  expect(() => resolveUiTheme(old, { contrast: "reject" })).toThrow(
    "theme.ui.low_contrast",
  );
  for (const color of [
    [17, 34, 51],
    [36, 53, 70],
    [34, 51, 68],
  ] as const)
    expect(
      resolveUiTheme(
        image("dark", "slate", "cyan", [{ slot: "bgBase", color }]),
        { contrast: "reject" },
      ).warnings,
    ).toEqual([]);
  expect(() =>
    resolveUiTheme(
      image("dark", "slate", "cyan", [
        { slot: "bgBase", color: [68, 85, 102] },
      ]),
      { contrast: "reject" },
    ),
  ).toThrow("theme.ui.low_contrast");
  for (const options of [null, [], Object.create({ administrative: true })])
    expect(() => resolveUiTheme(image(), options as never)).toThrow(
      "theme.ui.options_invalid",
    );
});
it("light fixed window data has independent UI appearance without modifying terminal colors", () => {
  const input = image("light", "slate", "cyan", [
    { slot: "bgBase", color: [249, 250, 251] },
    { slot: "accentFg", color: [13, 60, 72] },
    { slot: "warningFg", color: [79, 52, 34] },
    { slot: "successFg", color: [25, 59, 45] },
    { slot: "infoFg", color: [17, 50, 100] },
  ]);
  const stock = image("light", "slate", "cyan", [
    { slot: "bgBase", color: [249, 250, 251] },
  ]);
  expect(resolveUiTheme(stock).warnings.length).toBeGreaterThan(0);
  expect(() => resolveUiTheme(stock, { contrast: "reject" })).toThrow(
    "theme.ui.low_contrast",
  );
  const result = resolveUiTheme(input, { contrast: "reject" });
  expect(result.warnings).toEqual([]);
  expect(result.paint.terminal).toEqual(input.terminal);
  expect(colors(result.paint).fg).toBe(slate.slate12);
  expect(colors(result.paint).bgBase).toBe("#f9fafb");
});
it("administrative amber cannot be overridden by any imported accent slot", () => {
  const overrides = Object.keys(THEME_UI_PROPERTIES)
    .filter((k) => k.startsWith("accent"))
    .map((slot) => ({ slot, color: [255, 0, 255] }));
  overrides.push({ slot: "bgBase", color: [1, 2, 3] });
  const result = resolveUiTheme(image("dark", "slate", "#123456", overrides), {
    administrative: true,
  });
  const c = colors(result.paint);
  expect(result.paint.accent).toBe("amber");
  expect(c.accent).toBe(amberDark.amber9);
  expect(c.accentFg).toBe(amberDark.amber11);
  expect(c.bgBase).toBe("#010203");
  expect(result.ignoredAdministrativeOverrides).toBe(6);
});
it("custom RGB seeds are not silently approximated or mapped to stock palettes", () => {
  expect(() => resolveUiTheme(image("dark", "slate", "#123456"))).toThrow(
    "theme.ui.custom_accent_unsupported",
  );
});
it("WCAG sRGB reference values, symmetry and invalid channels", () => {
  expect(uiContrastRatio([255, 255, 255], [0, 0, 0])).toBe(21);
  expect(uiContrastRatio([255, 255, 255], [255, 255, 255])).toBe(1);
  expect(uiContrastRatio([255, 255, 255], [119, 119, 119])).toBeCloseTo(
    4.4780894536,
    9,
  );
  expect(uiContrastRatio([12, 34, 56], [210, 230, 250])).toBe(
    uiContrastRatio([210, 230, 250], [12, 34, 56]),
  );
  expect(() => uiContrastRatio([NaN, 0, 0], [0, 0, 0])).toThrow(
    "theme.paint.invalid",
  );
});
it("button text uses the stronger black/white choice against both default and hover", () => {
  for (const appearance of ["light", "dark"])
    for (const accent of RADIX_THEME_FAMILIES) {
      const r = resolveUiTheme(image(appearance, "slate", accent)).paint;
      const c = Object.fromEntries(r.overrides.map((v) => [v.slot, v.color]));
      if (!c.accentContrast || !c.accent || !c.accentHover)
        throw new Error("fixture missing fixed slot");
      const actual = Math.min(
        uiContrastRatio(c.accentContrast, c.accent),
        uiContrastRatio(c.accentContrast, c.accentHover),
      );
      for (const candidate of [
        [0, 0, 0],
        [255, 255, 255],
      ] as const)
        expect(actual).toBeGreaterThanOrEqual(
          Math.min(
            uiContrastRatio(candidate, c.accent),
            uiContrastRatio(candidate, c.accentHover),
          ),
        );
    }
});
it("untyped caller cannot inject selectors, extra options, unknown or duplicate color slots", () => {
  expect(() => resolveUiTheme({ ...image(), accent: "cyan;url(x)" })).toThrow(
    "theme.paint.invalid",
  );
  expect(() =>
    resolveUiTheme(image(), { administrative: "yes" } as never),
  ).toThrow("theme.ui.options_invalid");
  expect(() =>
    resolveUiTheme(image(), { contrast: "repair" } as never),
  ).toThrow("theme.ui.options_invalid");
  expect(() => resolveUiTheme(image(), { path: "PRIVATE" } as never)).toThrow(
    "theme.ui.options_invalid",
  );
  expect(() =>
    resolveUiTheme({
      ...image(),
      overrides: [{ slot: "constructor", color: [0, 0, 0] }],
    } as never),
  ).toThrow("theme.paint.invalid");
  expect(() =>
    resolveUiTheme({
      ...image(),
      overrides: [
        { slot: "fg", color: [0, 0, 0] },
        { slot: "fg", color: [0, 0, 0] },
      ],
    }),
  ).toThrow("theme.paint.invalid");
});
