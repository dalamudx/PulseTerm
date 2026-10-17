/** Fixed opaque color data only; no file, arbitrary CSS, view authority or DOM application. */
export const MAX_THEME_PAINT_BYTES = 4096;
export const THEME_UI_PROPERTIES = Object.freeze({
  bgBase: "--bg-base",
  bgSubtle: "--bg-subtle",
  bgSurface: "--bg-surface",
  bgHover: "--bg-hover",
  bgActive: "--bg-active",
  borderSubtle: "--border-subtle",
  border: "--border",
  borderStrong: "--border-strong",
  fgMuted: "--fg-muted",
  fg: "--fg",
  accentBg: "--accent-bg",
  accentBgHover: "--accent-bg-hover",
  accent: "--accent",
  accentHover: "--accent-hover",
  accentContrast: "--accent-contrast",
  accentFg: "--accent-fg",
  dangerBg: "--danger-bg",
  danger: "--danger",
  dangerFg: "--danger-fg",
  warningBg: "--warning-bg",
  warning: "--warning",
  warningFg: "--warning-fg",
  successBg: "--success-bg",
  success: "--success",
  successFg: "--success-fg",
  infoBg: "--info-bg",
  info: "--info",
  infoFg: "--info-fg",
});
export type UiColorSlot = keyof typeof THEME_UI_PROPERTIES;
export type Rgb8 = readonly [number, number, number];
export type Ansi16 = ReadonlyArray<Rgb8> & { readonly length: 16 };
export type ThemeAppearance = "light" | "dark";
export type NeutralScale =
  | "gray"
  | "mauve"
  | "slate"
  | "sage"
  | "olive"
  | "sand";
export interface TerminalPaint {
  readonly foreground: Rgb8;
  readonly background: Rgb8;
  readonly ansi: Ansi16;
  readonly cursor: Rgb8 | undefined;
  readonly cursorText: Rgb8 | undefined;
  readonly selectionBackground: Rgb8 | undefined;
  readonly selectionForeground: Rgb8 | undefined;
}
export interface ThemePaintData {
  readonly version: 1;
  readonly appearance: ThemeAppearance;
  readonly neutral: NeutralScale;
  /** Valid selector/seed only; not a generated Radix scale. */
  readonly accent: string;
  readonly terminal: TerminalPaint;
  readonly overrides: ReadonlyArray<
    Readonly<{ slot: UiColorSlot; color: Rgb8 }>
  >;
  /** Terminal foreground/background advisory only, not UI contrast verification. */
  readonly lowForegroundContrast: boolean;
}
export class ThemePaintError extends Error {
  constructor(
    readonly code:
      | "theme.paint.invalid"
      | "theme.paint.budget"
      | "theme.paint.version",
  ) {
    super(code);
    this.name = "ThemePaintError";
  }
}
function invalid(): never {
  throw new ThemePaintError("theme.paint.invalid");
}
function record(value: unknown): Record<string, unknown> {
  if (value === null || typeof value !== "object" || Array.isArray(value))
    return invalid();
  return value as Record<string, unknown>;
}
function keys(
  value: Record<string, unknown>,
  required: readonly string[],
  optional: readonly string[] = [],
): void {
  if (
    required.some((k) => !Object.hasOwn(value, k)) ||
    Object.keys(value).some(
      (k) => !required.includes(k) && !optional.includes(k),
    )
  )
    invalid();
}
function rgb(value: unknown): Rgb8 {
  if (
    !Array.isArray(value) ||
    value.length !== 3 ||
    value.some(
      (n) => typeof n !== "number" || !Number.isInteger(n) || n < 0 || n > 255,
    )
  )
    return invalid();
  return Object.freeze([value[0], value[1], value[2]]) as Rgb8;
}
function optionalRgb(
  value: Record<string, unknown>,
  key: string,
): Rgb8 | undefined {
  return Object.hasOwn(value, key) ? rgb(value[key]) : undefined;
}
const families = [
  "gray",
  "mauve",
  "slate",
  "sage",
  "olive",
  "sand",
  "tomato",
  "red",
  "ruby",
  "crimson",
  "pink",
  "plum",
  "purple",
  "violet",
  "iris",
  "indigo",
  "blue",
  "cyan",
  "teal",
  "jade",
  "green",
  "grass",
  "brown",
  "orange",
  "amber",
  "yellow",
  "lime",
  "mint",
  "sky",
  "gold",
  "bronze",
] as const;
/** Only compact canonical v1 JSON from the selected transport. Reject duplicate/escaped keys
 * and alternate numeric spellings via canonical roundtrip; never guess format or execute input.
 * Parsing and frozen values are not authorization: caller still needs current page/view/CSP guards.
 */
export function parseThemePaintData(encoded: unknown): ThemePaintData {
  if (typeof encoded !== "string") return invalid();
  if (
    encoded.length > MAX_THEME_PAINT_BYTES ||
    new TextEncoder().encode(encoded).length > MAX_THEME_PAINT_BYTES
  )
    throw new ThemePaintError("theme.paint.budget");
  let value: unknown;
  try {
    value = JSON.parse(encoded);
  } catch {
    return invalid();
  }
  const root = record(value);
  keys(root, [
    "version",
    "appearance",
    "neutral",
    "accent",
    "terminal",
    "overrides",
    "lowForegroundContrast",
  ]);
  if (root.version !== 1) throw new ThemePaintError("theme.paint.version");
  if (root.appearance !== "light" && root.appearance !== "dark")
    return invalid();
  if (
    typeof root.neutral !== "string" ||
    !["gray", "mauve", "slate", "sage", "olive", "sand"].includes(root.neutral)
  )
    return invalid();
  if (
    typeof root.accent !== "string" ||
    (!families.some((f) => f === root.accent) &&
      !/^#[0-9a-f]{6}$/.test(root.accent))
  )
    return invalid();
  if (typeof root.lowForegroundContrast !== "boolean") return invalid();
  const t = record(root.terminal);
  keys(
    t,
    ["foreground", "background", "ansi"],
    ["cursor", "cursorText", "selectionBackground", "selectionForeground"],
  );
  if (!Array.isArray(t.ansi) || t.ansi.length !== 16) return invalid();
  const terminal = Object.freeze({
    foreground: rgb(t.foreground),
    background: rgb(t.background),
    ansi: Object.freeze(t.ansi.map(rgb)) as Ansi16,
    cursor: optionalRgb(t, "cursor"),
    cursorText: optionalRgb(t, "cursorText"),
    selectionBackground: optionalRgb(t, "selectionBackground"),
    selectionForeground: optionalRgb(t, "selectionForeground"),
  });
  if (!Array.isArray(root.overrides) || root.overrides.length > 28)
    return invalid();
  const seen = new Set<string>();
  const overrides = Object.freeze(
    root.overrides.map((item) => {
      const color = record(item);
      keys(color, ["slot", "color"]);
      if (
        typeof color.slot !== "string" ||
        !Object.hasOwn(THEME_UI_PROPERTIES, color.slot) ||
        seen.has(color.slot)
      )
        return invalid();
      seen.add(color.slot);
      return Object.freeze({
        slot: color.slot as UiColorSlot,
        color: rgb(color.color),
      });
    }),
  );
  // Check canonical encoding only after depth/shape/array sizes are bounded.
  if (JSON.stringify(value) !== encoded) return invalid();
  return Object.freeze({
    version: 1,
    appearance: root.appearance,
    neutral: root.neutral as NeutralScale,
    accent: root.accent,
    terminal,
    overrides,
    lowForegroundContrast: root.lowForegroundContrast,
  });
}
/** Revalidates byte channels at the CSS-string boundary; no expression interpolation. */
export function rgbToHex(color: Rgb8): string {
  return `#${rgb(color)
    .map((n) => n.toString(16).padStart(2, "0"))
    .join("")}`;
}
