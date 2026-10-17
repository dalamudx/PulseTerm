// Validation-only, pinned xterm 6 browser internals. Not a production renderer API.
import type { Terminal } from "@xterm/xterm";
import type { QueryPaletteData } from "../terminal/query-palette";
import type { Rgb8 } from "../theme/paint-data";

type Color = { rgba: number };
/** The headless build has no ThemeService and MUST NOT pass this check. */
export function assertRenderedPalette(
  terminal: unknown,
  expected: QueryPaletteData,
): void {
  const colors = (
    terminal as {
      _core?: {
        _themeService?: {
          colors?: {
            ansi: Color[];
            foreground: Color;
            background: Color;
            cursor: Color;
          };
        };
      };
    }
  )?._core?._themeService?.colors;
  if (colors?.ansi.length !== 256)
    throw new Error("terminal.palette.browser_required");
  const compare = (color: Color | undefined, rgb: Rgb8) => {
    const rgba = color?.rgba;
    if (
      typeof rgba !== "number" ||
      !Number.isInteger(rgba) ||
      rgba < 0 ||
      rgba > 0xffffffff ||
      (rgba & 255) !== 255 ||
      rgba >>> 24 !== rgb[0] ||
      ((rgba >>> 16) & 255) !== rgb[1] ||
      ((rgba >>> 8) & 255) !== rgb[2]
    )
      throw new Error("terminal.palette.render_mismatch");
  };
  for (const [i, rgb] of expected.ansi.entries()) compare(colors.ansi[i], rgb);
  compare(colors.foreground, expected.foreground);
  compare(colors.background, expected.background);
  compare(colors.cursor, expected.cursor);
}
/** Runs only after the readonly snapshot's draw confirmation; RIS deliberately
 * destroys its display. The caller disposes that diagnostic surface afterward.
 * Keep disableStdin and the query suppression policy installed throughout. */
export async function verifyPaletteResets(
  terminal: Terminal,
  base: QueryPaletteData,
  signal: AbortSignal,
): Promise<void> {
  const write = (bytes: string) =>
    new Promise<void>((resolve, reject) => {
      if (signal.aborted) {
        reject(new Error("terminal.palette.retired"));
        return;
      }
      const abort = () => reject(new Error("terminal.palette.retired"));
      signal.addEventListener("abort", abort, { once: true });
      terminal.write(bytes, () => {
        signal.removeEventListener("abort", abort);
        if (signal.aborted) reject(new Error("terminal.palette.retired"));
        else resolve();
      });
    });
  await write(
    "\x18\x1b]4;1;#abcdef\x07\x1b]10;#102030\x07\x1b]11;#203040\x07\x1b]12;#304050\x07",
  );
  const ansi = base.ansi.map((c, i) =>
    i === 1 ? ([171, 205, 239] as const) : c,
  );
  const overridden: QueryPaletteData = {
    ...base,
    ansi: ansi as unknown as QueryPaletteData["ansi"],
    foreground: [16, 32, 48],
    background: [32, 48, 64],
    cursor: [48, 64, 80],
  };
  try {
    assertRenderedPalette(terminal, overridden);
  } catch {
    throw new Error("terminal.palette.override_mismatch");
  }
  await write("\x1b]104\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07");
  try {
    assertRenderedPalette(terminal, base);
  } catch {
    throw new Error("terminal.palette.reset_mismatch");
  }
  await write(
    "\x1b]4;1;#abcdef\x07\x1b]10;#102030\x07\x1b]11;#203040\x07\x1b]12;#304050\x07\x1bc",
  );
  try {
    // Backend and upstream xterm both retain remote OSC overrides across RIS.
    assertRenderedPalette(terminal, overridden);
  } catch {
    throw new Error("terminal.palette.ris_mismatch");
  }
  await write("\x1b]104\x07\x1b]110\x07\x1b]111\x07\x1b]112\x07");
  try {
    assertRenderedPalette(terminal, base);
  } catch {
    throw new Error("terminal.palette.reset_mismatch");
  }
}
