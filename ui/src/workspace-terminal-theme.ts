import { message, uiText } from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";
import {
  isTerminalPreset,
  renderTerminalPresetPreview,
  type TerminalPreset,
  type TerminalPresetPaint,
  terminalPresetLabels,
} from "./workspace-terminal-preset";
export type TerminalTheme = Readonly<{
  mode: "fixed" | "system";
  light: TerminalPreset;
  dark: TerminalPreset;
}>;
export function isTerminalTheme(input: unknown): input is TerminalTheme {
  if (!input || typeof input !== "object" || Array.isArray(input)) return false;
  const value = input as Record<string, unknown>;
  return (
    Object.keys(value).length === 3 &&
    ["mode", "light", "dark"].every((key) => Object.hasOwn(value, key)) &&
    (value.mode === "fixed" || value.mode === "system") &&
    isTerminalPreset(value.light) &&
    isTerminalPreset(value.dark)
  );
}
export function terminalThemeField(options: {
  document: Document;
  setting: DisplaySetting<TerminalTheme>;
  presets: readonly TerminalPresetPaint[];
  signal: AbortSignal;
  current: () => boolean;
  save: (change: SettingsChange) => void;
}): HTMLFormElement {
  const { document: doc, signal } = options;
  const row = doc.createElement("form"),
    label = doc.createElement("label"),
    mode = doc.createElement("select"),
    save = doc.createElement("button"),
    reset = doc.createElement("button"),
    source = doc.createElement("span"),
    help = doc.createElement("p"),
    choices = doc.createElement("div");
  row.className = "workspace-setting-row";
  uiText(label, "终端配色方式");
  label.append(mode);
  for (const [id, text] of [
    ["fixed", "固定配色"],
    ["system", "跟随系统"],
  ]) {
    const option = doc.createElement("option");
    option.value = id ?? "";
    uiText(option, text ?? "");
    mode.append(option);
  }
  mode.value = options.setting.value.mode;
  choices.className = "terminal-theme-choices";
  const selects: HTMLSelectElement[] = [];
  const previews: HTMLPreElement[] = [];
  for (const [key, title] of [
    ["light", "系统浅色主题"],
    ["dark", "系统深色主题"],
  ] as const) {
    const pane = doc.createElement("div"),
      name = doc.createElement("label"),
      select = doc.createElement("select"),
      preview = doc.createElement("pre");
    uiText(name, title);
    name.append(select);
    for (const paint of options.presets) {
      const option = doc.createElement("option");
      option.value = paint.id;
      uiText(option, terminalPresetLabels[paint.id]);
      select.append(option);
    }
    select.value = options.setting.value[key];
    preview.className = "terminal-preset-preview";
    pane.append(name, preview);
    choices.append(pane);
    selects.push(select);
    previews.push(preview);
  }
  const current = () => !signal.aborted && options.current();
  const value = (): TerminalTheme | undefined => {
    const draft = {
      mode: mode.value,
      light: selects[0]?.value,
      dark: selects[1]?.value,
    };
    return isTerminalTheme(draft) &&
      [draft.light, draft.dark].every((id) =>
        options.presets.some((paint) => paint.id === id),
      )
      ? draft
      : undefined;
  };
  const render = () => {
    if (!current()) return;
    choices.hidden = mode.value !== "system";
    selects.forEach((select, index) => {
      select.disabled = choices.hidden;
      const paint = options.presets.find((item) => item.id === select.value),
        preview = previews[index];
      if (paint && preview) renderTerminalPresetPreview(doc, preview, paint);
    });
  };
  mode.addEventListener("change", render, { signal });
  for (const select of selects)
    select.addEventListener("change", render, { signal });
  save.type = "submit";
  uiText(save, "保存");
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiText(
    source,
    message(
      "当前：{0} · {1}",
      message(
        options.setting.value.mode === "system" ? "跟随系统" : "固定配色",
      ),
      message(options.setting.source === "user" ? "用户设置" : "默认"),
    ),
  );
  help.className = "terminal-preset-help";
  uiText(
    help,
    "跟随系统在新建会话时选择浅色或深色主题；Profile、分组或 SSH 入口的固定配色优先。已有会话保持原配色。",
  );
  row.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      const draft = value();
      if (current() && draft)
        options.save({ field: "terminal_theme", value: draft });
    },
    { signal },
  );
  reset.addEventListener(
    "click",
    () => {
      if (current()) options.save({ field: "reset_terminal_theme" });
    },
    { signal },
  );
  row.append(label, save, reset, source, choices, help);
  render();
  return row;
}
