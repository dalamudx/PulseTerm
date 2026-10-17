import { message, uiAttribute, uiText } from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";

export const terminalPresetIds = [
  "classic",
  "midnight",
  "paper",
  "one_dark",
  "one_light",
  "catppuccin_mocha",
  "catppuccin_latte",
  "dracula",
  "solarized_dark",
  "solarized_light",
] as const;
export type TerminalPreset = (typeof terminalPresetIds)[number];
type Rgb = readonly [number, number, number];
export interface TerminalPresetPaint {
  readonly id: TerminalPreset;
  readonly colors: Readonly<{
    ansi: readonly Rgb[];
    foreground: Rgb;
    background: Rgb;
    cursor: Rgb;
  }>;
}
const labels = {
  classic: "经典黑底",
  midnight: "PulseTerm Dark（午夜）",
  paper: "PulseTerm Light（纸白）",
  one_dark: "One Dark",
  one_light: "One Light",
  catppuccin_mocha: "Catppuccin Mocha（深色）",
  catppuccin_latte: "Catppuccin Latte（浅色）",
  dracula: "Dracula（深色）",
  solarized_dark: "Solarized Dark",
  solarized_light: "Solarized Light",
} as const satisfies Record<TerminalPreset, string>;

export { labels as terminalPresetLabels };
export function isTerminalPreset(value: unknown): value is TerminalPreset {
  return typeof value === "string" && Object.hasOwn(labels, value);
}
function invalid(): never {
  throw Error("workspace.settings_invalid_projection");
}
function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    return invalid();
  const result = value as Record<string, unknown>;
  if (
    Object.keys(result).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(result, key))
  )
    return invalid();
  return result;
}
function rgb(value: unknown): Rgb {
  if (
    !Array.isArray(value) ||
    value.length !== 3 ||
    value.some(
      (v) => typeof v !== "number" || !Number.isInteger(v) || v < 0 || v > 255,
    )
  )
    return invalid();
  return Object.freeze([value[0], value[1], value[2]]) as Rgb;
}
export function decodeTerminalPresets(
  value: unknown,
  catalog: "legacy" | "builtin" = "legacy",
): readonly TerminalPresetPaint[] {
  const ids =
    catalog === "builtin" ? terminalPresetIds : terminalPresetIds.slice(0, 3);
  if (!Array.isArray(value) || value.length !== ids.length) return invalid();
  const seen = new Set<TerminalPreset>();
  return Object.freeze(
    value.map((entry) => {
      const paint = record(entry, ["id", "colors"]);
      if (
        !isTerminalPreset(paint.id) ||
        !ids.some((id) => id === paint.id) ||
        seen.has(paint.id)
      )
        return invalid();
      seen.add(paint.id);
      const colors = record(paint.colors, [
        "ansi",
        "foreground",
        "background",
        "cursor",
      ]);
      if (!Array.isArray(colors.ansi) || colors.ansi.length !== 16)
        return invalid();
      return Object.freeze({
        id: paint.id,
        colors: Object.freeze({
          ansi: Object.freeze(colors.ansi.map(rgb)),
          foreground: rgb(colors.foreground),
          background: rgb(colors.background),
          cursor: rgb(colors.cursor),
        }),
      });
    }),
  );
}
const css = (color: Rgb): string => `rgb(${color.join(", ")})`;
export function renderTerminalPresetPreview(
  doc: Document,
  preview: HTMLPreElement,
  paint: TerminalPresetPaint,
): void {
  preview.style.backgroundColor = css(paint.colors.background);
  preview.style.color = css(paint.colors.foreground);
  uiAttribute(
    preview,
    "aria-label",
    message("{0}配色预览", message(labels[paint.id])),
  );
  // Drawing sample data stays identical in both interface languages.
  const sample = "AaBb 0123 · 中文 かな 한글\n";
  preview.textContent = sample;
  [
    "普通 ",
    "错误 ",
    "成功 ",
    "警告 ",
    "链接 ",
    "强调 ",
    "提示 ",
    "文本",
  ].forEach((text, index) => {
    const color = paint.colors.ansi[index];
    if (!color) return;
    const span = doc.createElement("span"),
      swatch = doc.createElement("span"),
      description = doc.createElement("span");
    swatch.className = "workspace-preset-swatch";
    swatch.setAttribute("aria-hidden", "true");
    swatch.style.backgroundColor = css(color);
    uiText(description, text);
    span.append(swatch, description);
    preview.append(span);
  });
}
export function terminalPresetField(options: {
  document: Document;
  setting: DisplaySetting<TerminalPreset>;
  presets: readonly TerminalPresetPaint[];
  systemTheme?: boolean;
  signal: AbortSignal;
  current: () => boolean;
  save: (change: SettingsChange) => void;
}): HTMLFormElement {
  const { document: doc, signal } = options;
  const row = doc.createElement("form"),
    label = doc.createElement("label"),
    select = doc.createElement("select"),
    save = doc.createElement("button"),
    reset = doc.createElement("button"),
    source = doc.createElement("span"),
    preview = doc.createElement("pre"),
    help = doc.createElement("p");
  row.className = "workspace-setting-row";
  uiText(label, "终端配色");
  label.append(select);
  for (const paint of options.presets) {
    const option = doc.createElement("option");
    option.value = paint.id;
    uiText(option, labels[paint.id]);
    select.append(option);
  }
  select.value = options.setting.value;
  save.type = "submit";
  uiText(save, "保存");
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiText(
    source,
    message(
      "当前：{0} · {1}",
      message(labels[options.setting.value]),
      message(options.setting.source === "user" ? "用户设置" : "默认"),
    ),
  );
  uiText(
    help,
    options.systemTheme
      ? "跟随系统已启用；切回固定配色后，新建终端使用此配色。已有终端保持原配色。"
      : "保存后用于新建本地或 SSH 终端，已有终端保持原配色。",
  );
  help.className = "terminal-preset-help";
  preview.className = "terminal-preset-preview";
  const selected = () =>
    options.presets.find((paint) => paint.id === select.value);
  const render = () => {
    if (signal.aborted || !options.current()) return;
    const paint = selected();
    if (!paint) return;
    renderTerminalPresetPreview(doc, preview, paint);
  };
  select.addEventListener("change", render, { signal });
  row.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      const paint = selected();
      if (!signal.aborted && options.current() && paint)
        options.save({ field: "terminal_preset", value: paint.id });
    },
    { signal },
  );
  reset.addEventListener(
    "click",
    () => {
      if (!signal.aborted && options.current())
        options.save({ field: "reset_terminal_preset" });
    },
    { signal },
  );
  row.append(label, save, reset, source, preview, help);
  render();
  return row;
}
