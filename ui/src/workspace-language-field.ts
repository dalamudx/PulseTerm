import type { UiLanguage } from "./workspace-language";
import {
  isUiLanguage,
  message,
  uiAttribute,
  uiText,
} from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";
export function languageField(options: {
  document: Document;
  setting: DisplaySetting<UiLanguage>;
  signal: AbortSignal;
  current(): boolean;
  save(change: SettingsChange): void;
}): HTMLFormElement {
  const doc = options.document,
    row = doc.createElement("form"),
    label = doc.createElement("label"),
    select = doc.createElement("select"),
    source = doc.createElement("span"),
    hint = doc.createElement("p"),
    save = doc.createElement("button"),
    reset = doc.createElement("button");
  row.className = "workspace-setting-row";
  uiText(label, "界面语言");
  uiAttribute(select, "aria-label", "界面语言");
  for (const [id, autonym] of [
    ["zh-CN", "简体中文"],
    ["en", "English"],
  ] as const) {
    const option = doc.createElement("option");
    option.value = id;
    option.textContent = autonym;
    select.append(option);
  }
  select.value = options.setting.value;
  label.append(select);
  source.className = "workspace-setting-source";
  uiText(source, options.setting.source === "user" ? "用户覆盖" : "内置默认");
  hint.className = "terminal-preset-help";
  uiText(hint, "语言保存后立即应用；终端输出和用户数据保持原样。");
  save.type = "submit";
  uiText(save, "保存");
  uiAttribute(save, "aria-label", message("保存{0}", message("界面语言")));
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiAttribute(reset, "aria-label", message("{0}恢复继承", message("界面语言")));
  row.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      if (
        !options.signal.aborted &&
        options.current() &&
        isUiLanguage(select.value)
      )
        options.save({ field: "ui_language", value: select.value });
    },
    { signal: options.signal },
  );
  reset.addEventListener(
    "click",
    () => {
      if (!options.signal.aborted && options.current())
        options.save({ field: "reset_ui_language" });
    },
    { signal: options.signal },
  );
  row.append(label, source, save, reset, hint);
  return row;
}
