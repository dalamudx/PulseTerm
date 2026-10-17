import { message, uiAttribute, uiText } from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";

const accents = {
  cyan: "青色",
  blue: "蓝色",
  green: "绿色",
  amber: "琥珀色",
  red: "红色",
} as const;
export type UiAccent = keyof typeof accents;
export function isUiAccent(value: unknown): value is UiAccent {
  return typeof value === "string" && Object.hasOwn(accents, value);
}
/** Fixed data attribute selects shipped CSS; no arbitrary CSS or terminal authority. */
export function applyUiAccent(
  document: Document,
  value: unknown,
  signal: AbortSignal,
): boolean {
  if (signal.aborted || !isUiAccent(value)) return false;
  document.documentElement.setAttribute("data-ui-accent", value);
  return true;
}
export function uiAccentField(options: {
  document: Document;
  setting: DisplaySetting<UiAccent>;
  signal: AbortSignal;
  current(): boolean;
  save(change: SettingsChange): void;
}): HTMLFormElement {
  const { document: doc, signal } = options;
  const row = doc.createElement("form"),
    label = doc.createElement("label"),
    select = doc.createElement("select"),
    source = doc.createElement("span"),
    save = doc.createElement("button"),
    reset = doc.createElement("button"),
    preview = doc.createElement("section"),
    sample = doc.createElement("strong"),
    selected = doc.createElement("span"),
    help = doc.createElement("p");
  row.className = "workspace-setting-row";
  uiText(label, "界面强调色");
  label.append(select);
  for (const [value, text] of Object.entries(accents)) {
    const option = doc.createElement("option");
    option.value = value;
    uiText(option, text);
    select.append(option);
  }
  select.value = options.setting.value;
  source.className = "workspace-setting-source";
  uiText(
    source,
    message(
      "当前：{0} · {1}",
      message(accents[options.setting.value]),
      message(options.setting.source === "user" ? "用户设置" : "内置默认"),
    ),
  );
  save.type = "submit";
  uiText(save, "保存");
  uiAttribute(save, "aria-label", "保存界面强调色");
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiAttribute(reset, "aria-label", "恢复界面强调色继承");
  preview.className = "workspace-accent-preview";
  uiText(sample, "AaBb 中文 123");
  uiText(selected, "选中项目");
  preview.append(sample, selected);
  help.className = "terminal-preset-help";
  uiText(help, "仅预览 · 保存后应用到界面，终端配色保持独立。");
  const current = () => !signal.aborted && options.current();
  const render = () => {
    if (!current() || !isUiAccent(select.value)) return;
    preview.setAttribute("data-ui-accent", select.value);
    uiAttribute(
      preview,
      "aria-label",
      message("{0}强调色预览", message(accents[select.value])),
    );
  };
  select.addEventListener("change", render, { signal });
  row.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      if (current() && isUiAccent(select.value))
        options.save({ field: "ui_accent", value: select.value });
    },
    { signal },
  );
  reset.addEventListener(
    "click",
    () => {
      if (current()) options.save({ field: "reset_ui_accent" });
    },
    { signal },
  );
  row.append(label, source, save, reset, preview, help);
  render();
  return row;
}
