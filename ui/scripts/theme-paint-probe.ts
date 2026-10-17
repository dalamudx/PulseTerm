import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";
import {
  parseQueryPaletteData,
  queryPaletteTheme,
} from "../src/features/terminal/query-palette";
import {
  parseThemePaintData,
  rgbToHex,
} from "../src/features/theme/paint-data";
import { resolveUiTheme } from "../src/features/theme/radix-theme";

const executable = fileURLToPath(
  new URL(
    `../../target/debug/xtask${process.platform === "win32" ? ".exe" : ""}`,
    import.meta.url,
  ),
);
const child = spawnSync(executable, ["probe-theme-paint-json"], {
  encoding: "utf8",
  maxBuffer: 64 * 1024,
  timeout: 10000,
});
if (
  child.error ||
  child.signal ||
  child.status !== 0 ||
  typeof child.stdout !== "string"
)
  throw new Error("theme.paint.backend_failed");
const rows = child.stdout
  .split(/\r?\n/)
  .filter((line) => line.startsWith("THEME_PAINT "))
  .map((line) => line.slice("THEME_PAINT ".length));
if (rows.length !== 14) throw new Error("theme.paint.backend_count");
const queryRows = child.stdout
  .split(/\r?\n/)
  .filter((line) => line.startsWith("QUERY_PALETTE "))
  .map((line) => line.slice("QUERY_PALETTE ".length));
if (queryRows.length !== 14) throw new Error("theme.paint.palette_count");
const appearances = new Set<string>();
let mappedOverrides = 0;
for (const [index, row] of rows.entries()) {
  const data = parseThemePaintData(row);
  const queryRow = queryRows[index];
  if (!queryRow) throw new Error("theme.paint.palette_missing");
  const palette = parseQueryPaletteData(queryRow),
    options = queryPaletteTheme(palette);
  const expected = data.terminal;
  if (
    JSON.stringify(palette.ansi) !== JSON.stringify(expected.ansi) ||
    JSON.stringify(palette.foreground) !==
      JSON.stringify(expected.foreground) ||
    JSON.stringify(palette.background) !==
      JSON.stringify(expected.background) ||
    JSON.stringify(palette.cursor) !==
      JSON.stringify(expected.cursor ?? expected.foreground) ||
    Object.keys(options).length !== 19 ||
    options.foreground !== rgbToHex(palette.foreground)
  )
    throw new Error("theme.paint.palette_diverged");
  appearances.add(data.appearance);
  mappedOverrides += data.overrides.length;
  const resolved = resolveUiTheme(data);
  if (
    resolved.paint.overrides.length !== 28 ||
    resolved.warnings.length > 25 ||
    JSON.stringify(resolved.paint.terminal) !== JSON.stringify(data.terminal) ||
    JSON.stringify(parseThemePaintData(JSON.stringify(resolved.paint))) !==
      JSON.stringify(resolved.paint)
  )
    throw new Error("theme.paint.radix_shape");
  for (const original of data.overrides) {
    const generated = resolved.paint.overrides.find(
      (v) => v.slot === original.slot,
    );
    if (!generated || rgbToHex(generated.color) !== rgbToHex(original.color))
      throw new Error("theme.paint.override_lost");
  }
  if (
    data.terminal.ansi.length !== 16 ||
    data.neutral !== "slate" ||
    data.accent !== "cyan"
  )
    throw new Error("theme.paint.backend_shape");
  for (const color of data.terminal.ansi) {
    if (!/^#[0-9a-f]{6}$/.test(rgbToHex(color)))
      throw new Error("theme.paint.rgb_encoding");
  }
}
if (queryRows.length !== 14 || appearances.size !== 2 || mappedOverrides !== 4)
  throw new Error("theme.paint.backend_coverage");
console.log(
  "PASS theme-paint: 14 actual Rust theme documents → strict UI Radix28 and terminal19 capture parser, base RGB equivalence and cursor fallback; actual browser reset checked separately; no user files",
);
