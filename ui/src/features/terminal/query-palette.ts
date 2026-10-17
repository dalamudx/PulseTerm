// Drawing data only. A palette is not a session/window/generation capability.
import type { ITheme } from "@xterm/headless";
import { type Ansi16, type Rgb8, rgbToHex } from "../theme/paint-data";
export const MAX_QUERY_PALETTE_BYTES = 1024;
export interface QueryPaletteData {
  readonly ansi: Ansi16;
  readonly foreground: Rgb8;
  readonly background: Rgb8;
  readonly cursor: Rgb8;
}
const ANSI = [
  "black",
  "red",
  "green",
  "yellow",
  "blue",
  "magenta",
  "cyan",
  "white",
  "brightBlack",
  "brightRed",
  "brightGreen",
  "brightYellow",
  "brightBlue",
  "brightMagenta",
  "brightCyan",
  "brightWhite",
] as const;
function invalid(): never {
  throw new Error("terminal.palette.invalid");
}
function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return invalid();
  const entries = Object.keys(value);
  if (
    entries.length !== keys.length ||
    !entries.every((key) => keys.includes(key))
  )
    return invalid();
  return value as Record<string, unknown>;
}
function rgb(value: unknown): Rgb8 {
  if (
    !Array.isArray(value) ||
    value.length !== 3 ||
    !value.every((v) => Number.isInteger(v) && v >= 0 && v <= 255)
  )
    return invalid();
  return Object.freeze([value[0], value[1], value[2]]) as Rgb8;
}
/** Validation-only v1 envelope, separate from unchanged binary attachment v1/v2. */
export function parseQueryPaletteData(encoded: string): QueryPaletteData {
  if (typeof encoded !== "string" || encoded.length > MAX_QUERY_PALETTE_BYTES)
    return invalid();
  let value: unknown;
  try {
    value = JSON.parse(encoded);
  } catch {
    return invalid();
  }
  const root = record(value, ["version", "palette"]);
  if (root.version !== 1) return invalid();
  const palette = record(root.palette, [
    "ansi",
    "foreground",
    "background",
    "cursor",
  ]);
  if (!Array.isArray(palette.ansi) || palette.ansi.length !== 16)
    return invalid();
  const ansi = Object.freeze(palette.ansi.map(rgb)) as Ansi16;
  if (JSON.stringify(value) !== encoded) return invalid();
  return Object.freeze({
    ansi,
    foreground: rgb(palette.foreground),
    background: rgb(palette.background),
    cursor: rgb(palette.cursor),
  });
}
/** Constructor base theme, applied BEFORE History/Restore. Never emit OSC base
 * colors: those would become overrides and break OSC104/110/111/112 and RIS. */
export function queryPaletteTheme(data: QueryPaletteData): Readonly<ITheme> {
  const palette = parseQueryPaletteData(
    JSON.stringify({ version: 1, palette: data }),
  );
  const theme: ITheme = {
    foreground: rgbToHex(palette.foreground),
    background: rgbToHex(palette.background),
    cursor: rgbToHex(palette.cursor),
  };
  for (const [index, key] of ANSI.entries()) {
    const color = palette.ansi[index];
    if (!color) return invalid();
    theme[key] = rgbToHex(color);
  }
  return Object.freeze(theme);
}
