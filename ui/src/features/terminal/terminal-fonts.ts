import type { DisplayPreferences } from "./display-preferences";
import { BUNDLED_TERMINAL_FONT } from "./font-presets";

/** Local bundled face only. This is drawing preparation, never view/input authority. */
export async function waitForBundledTerminalFont(
  doc: Document,
  display: Readonly<DisplayPreferences>,
  signal: AbortSignal,
): Promise<void> {
  if (signal.aborted) throw Error("view.retired");
  if (
    display.fontFamily !== BUNDLED_TERMINAL_FONT &&
    display.fontFallback !== BUNDLED_TERMINAL_FONT
  )
    return;
  const fonts = doc.fonts;
  if (!fonts?.load || !fonts.check) throw Error("terminal.font_unavailable");
  const font = `${display.fontSize}px "${BUNDLED_TERMINAL_FONT}"`;
  await new Promise<void>((resolve, reject) => {
    let settled = false;
    const finish = (error?: Error) => {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      signal.removeEventListener("abort", aborted);
      if (error) reject(error);
      else resolve();
    };
    const aborted = () => finish(Error("view.retired"));
    const timer = setTimeout(
      () => finish(Error("terminal.font_timeout")),
      1500,
    );
    signal.addEventListener("abort", aborted, { once: true });
    if (signal.aborted) {
      aborted();
      return;
    }
    void Promise.resolve()
      .then(() => {
        if (signal.aborted) throw Error("view.retired");
        return fonts.load(font, "M");
      })
      .then(
        (faces) => {
          if (signal.aborted) aborted();
          else if (!faces.length || !fonts.check(font, "M"))
            finish(Error("terminal.font_unavailable"));
          else finish();
        },
        () => finish(Error("terminal.font_unavailable")),
      );
  });
}

export function terminalFontFamily(
  display: Readonly<DisplayPreferences>,
): string {
  if (
    display.fontFallback === undefined &&
    display.fontFamily !== BUNDLED_TERMINAL_FONT
  )
    return display.fontFamily;
  const generic = new Set([
    "monospace",
    "ui-monospace",
    "serif",
    "sans-serif",
    "cursive",
    "fantasy",
    "system-ui",
    "ui-serif",
    "ui-sans-serif",
    "ui-rounded",
    "math",
    "emoji",
    "fangsong",
  ]);
  return [
    ...new Set([
      display.fontFamily,
      display.fontFallback ?? "monospace",
      "monospace",
    ]),
  ]
    .map((name) => (generic.has(name.toLowerCase()) ? name : `"${name}"`))
    .join(", ");
}
