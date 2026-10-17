import {
  amber,
  amberDark,
  blue,
  blueDark,
  bronze,
  bronzeDark,
  brown,
  brownDark,
  crimson,
  crimsonDark,
  cyan,
  cyanDark,
  gold,
  goldDark,
  grass,
  grassDark,
  gray,
  grayDark,
  green,
  greenDark,
  indigo,
  indigoDark,
  iris,
  irisDark,
  jade,
  jadeDark,
  lime,
  limeDark,
  mauve,
  mauveDark,
  mint,
  mintDark,
  olive,
  oliveDark,
  orange,
  orangeDark,
  pink,
  pinkDark,
  plum,
  plumDark,
  purple,
  purpleDark,
  red,
  redDark,
  ruby,
  rubyDark,
  sage,
  sageDark,
  sand,
  sandDark,
  sky,
  skyDark,
  slate,
  slateDark,
  teal,
  tealDark,
  tomato,
  tomatoDark,
  violet,
  violetDark,
  yellow,
  yellowDark,
} from "@radix-ui/colors";
import {
  parseThemePaintData,
  type Rgb8,
  rgbToHex,
  THEME_UI_PROPERTIES,
  type ThemePaintData,
  type UiColorSlot,
} from "./paint-data";

// Only opaque sRGB light/dark exports. No alpha/P3, CSS expressions or hand-built scale.
const SCALES = {
  gray: [gray, grayDark],
  mauve: [mauve, mauveDark],
  slate: [slate, slateDark],
  sage: [sage, sageDark],
  olive: [olive, oliveDark],
  sand: [sand, sandDark],
  tomato: [tomato, tomatoDark],
  red: [red, redDark],
  ruby: [ruby, rubyDark],
  crimson: [crimson, crimsonDark],
  pink: [pink, pinkDark],
  plum: [plum, plumDark],
  purple: [purple, purpleDark],
  violet: [violet, violetDark],
  iris: [iris, irisDark],
  indigo: [indigo, indigoDark],
  blue: [blue, blueDark],
  cyan: [cyan, cyanDark],
  teal: [teal, tealDark],
  jade: [jade, jadeDark],
  green: [green, greenDark],
  grass: [grass, grassDark],
  brown: [brown, brownDark],
  orange: [orange, orangeDark],
  amber: [amber, amberDark],
  yellow: [yellow, yellowDark],
  lime: [lime, limeDark],
  mint: [mint, mintDark],
  sky: [sky, skyDark],
  gold: [gold, goldDark],
  bronze: [bronze, bronzeDark],
} as const;
export const RADIX_THEME_FAMILIES = Object.freeze(
  Object.keys(SCALES),
) as readonly (keyof typeof SCALES)[];
export const UI_TEXT_MIN_CONTRAST = 4.5;
export interface UiContrastWarning {
  readonly foreground: UiColorSlot;
  readonly background: UiColorSlot;
  readonly ratio: number;
}
export interface UiThemeOptions {
  /** Trusted window metadata only, never a field from an imported theme. */
  readonly administrative?: boolean;
  /** Warn preserves imported colors. Reject is explicit apply-time protection, never adjustment. */
  readonly contrast?: "warn" | "reject";
}
export interface ResolvedUiTheme {
  /** Exactly 28 fixed UI slots. Terminal coordinates and colors are untouched. */
  readonly paint: ThemePaintData;
  readonly warnings: readonly UiContrastWarning[];
  readonly ignoredAdministrativeOverrides: number;
}
function hex(value: string): Rgb8 {
  if (!/^#[0-9a-f]{6}$/i.test(value))
    throw new Error("theme.ui.palette_invalid");
  return Object.freeze([
    Number.parseInt(value.slice(1, 3), 16),
    Number.parseInt(value.slice(3, 5), 16),
    Number.parseInt(value.slice(5, 7), 16),
  ]);
}
function luminance(color: Rgb8): number {
  const [r, g, b] = color.map((v) => {
    const c = v / 255;
    return c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  });
  return 0.2126 * (r ?? 0) + 0.7152 * (g ?? 0) + 0.0722 * (b ?? 0);
}
/** WCAG 2 sRGB relative luminance. Revalidate tuple channels even for untyped callers. */
export function uiContrastRatio(foreground: Rgb8, background: Rgb8): number {
  rgbToHex(foreground);
  rgbToHex(background);
  const a = luminance(foreground),
    b = luminance(background);
  return (Math.max(a, b) + 0.05) / (Math.min(a, b) + 0.05);
}
const PAIRS: readonly (readonly [UiColorSlot, UiColorSlot])[] = Object.freeze([
  ...(["fg", "fgMuted"] as const).flatMap((f) =>
    (["bgBase", "bgSubtle", "bgSurface", "bgHover", "bgActive"] as const).map(
      (b) => [f, b] as const,
    ),
  ),
  ["fg", "accentBg"],
  ["fg", "accentBgHover"],
  ["accentFg", "bgBase"],
  ["accentFg", "accentBg"],
  ["accentFg", "accentBgHover"],
  ["accentContrast", "accent"],
  ["accentContrast", "accentHover"],
  ["dangerFg", "bgBase"],
  ["dangerFg", "dangerBg"],
  ["warningFg", "bgBase"],
  ["warningFg", "warningBg"],
  ["successFg", "bgBase"],
  ["successFg", "successBg"],
  ["infoFg", "bgBase"],
  ["infoFg", "infoBg"],
]);
/** Resolves stock Radix3 scales only, with no DOM/authority. RGB seeds fail explicitly, never guessed. */
export function resolveUiTheme(
  image: ThemePaintData,
  options: UiThemeOptions = {},
): ResolvedUiTheme {
  const data = parseThemePaintData(JSON.stringify(image));
  if (
    options === null ||
    typeof options !== "object" ||
    Array.isArray(options) ||
    ![null, Object.prototype].includes(Object.getPrototypeOf(options)) ||
    Object.keys(options).some(
      (k) => k !== "administrative" && k !== "contrast",
    ) ||
    (options.administrative !== undefined &&
      typeof options.administrative !== "boolean") ||
    (options.contrast !== undefined &&
      options.contrast !== "warn" &&
      options.contrast !== "reject")
  )
    throw new Error("theme.ui.options_invalid");
  const family = options.administrative ? "amber" : data.accent;
  if (family.startsWith("#"))
    throw new Error("theme.ui.custom_accent_unsupported");
  if (!Object.hasOwn(SCALES, family))
    throw new Error("theme.ui.palette_invalid");
  const appearance = data.appearance === "dark" ? 1 : 0;
  const step = (name: keyof typeof SCALES, index: number): Rgb8 => {
    const scale: Readonly<Record<string, string>> = SCALES[name][appearance];
    return hex(scale[`${name}${index}`] ?? "");
  };
  const n = (index: number) => step(data.neutral, index),
    a = (index: number) => step(family as keyof typeof SCALES, index);
  const white = hex("#ffffff"),
    black = hex("#000000");
  // Maximize the lower contrast against both button states; explicit black/white, no RGB guessing.
  const candidate = (color: Rgb8) =>
    Math.min(uiContrastRatio(color, a(9)), uiContrastRatio(color, a(10)));
  const contrast = candidate(white) >= candidate(black) ? white : black;
  const colors: Record<UiColorSlot, Rgb8> = {
    bgBase: n(1),
    bgSubtle: n(2),
    bgSurface: n(3),
    bgHover: n(4),
    bgActive: n(5),
    borderSubtle: n(6),
    border: n(7),
    borderStrong: n(8),
    fgMuted: n(11),
    fg: n(12),
    accentBg: a(3),
    accentBgHover: a(4),
    accent: a(9),
    accentHover: a(10),
    accentContrast: contrast,
    accentFg: a(11),
    dangerBg: step("red", 3),
    danger: step("red", 9),
    dangerFg: step("red", 11),
    warningBg: step("amber", 3),
    warning: step("amber", 9),
    warningFg: step("amber", 11),
    successBg: step("green", 3),
    success: step("green", 9),
    successFg: step("green", 11),
    infoBg: step("blue", 3),
    info: step("blue", 9),
    infoFg: step("blue", 11),
  };
  let ignored = 0;
  for (const override of data.overrides) {
    if (options.administrative && override.slot.startsWith("accent")) {
      ignored++;
      continue;
    }
    colors[override.slot] = override.color;
  }
  const warnings = Object.freeze(
    PAIRS.flatMap(([foreground, background]) => {
      const ratio = uiContrastRatio(colors[foreground], colors[background]);
      return ratio < UI_TEXT_MIN_CONTRAST
        ? [Object.freeze({ foreground, background, ratio })]
        : [];
    }),
  );
  if (options.contrast === "reject" && warnings.length)
    throw new Error("theme.ui.low_contrast");
  const overrides = Object.freeze(
    (Object.keys(THEME_UI_PROPERTIES) as UiColorSlot[]).map((slot) =>
      Object.freeze({ slot, color: colors[slot] }),
    ),
  );
  const paint = Object.freeze({ ...data, accent: family, overrides });
  return Object.freeze({
    paint,
    warnings,
    ignoredAdministrativeOverrides: ignored,
  });
}
