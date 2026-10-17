import { message, uiAttribute, uiText } from "./workspace-language";
import {
  type DisplaySetting,
  type SettingsChange,
  validateSettingsChange,
} from "./workspace-settings";
/** Closed settings controls; no session or input authority. */
export function terminalBehaviorField(options: {
  document: Document;
  field: "delete_send" | "term_type";
  setting: DisplaySetting<string>;
  signal: AbortSignal;
  current(): boolean;
  save(change: SettingsChange): void;
}): HTMLFormElement {
  const { document: doc, field, setting } = options;
  const form = doc.createElement("form"),
    label = doc.createElement("label"),
    select = doc.createElement("select"),
    source = doc.createElement("span"),
    save = doc.createElement("button"),
    reset = doc.createElement("button");
  const term = field === "term_type";
  const caption = term
    ? "TERM（仅新建会话；不切换终端模拟器）"
    : "Delete 发送编码（下次显示生效）";
  uiText(label, caption);
  uiAttribute(select, "aria-label", caption);
  const choices = term
    ? [
        ["xterm-256color", "xterm-256color（默认）"],
        ["xterm", "xterm"],
        ["vt100", "vt100"],
      ]
    : [
        ["csi", "ESC [ 3 ~（默认）"],
        ["del", "^?（DEL）"],
      ];
  for (const [value, text] of choices) {
    const item = doc.createElement("option");
    item.value = value ?? "";
    uiText(item, text ?? "");
    select.append(item);
  }
  select.value = setting.value;
  label.append(select);
  uiText(
    source,
    message(
      "来源：{0}",
      message(setting.source === "user" ? "用户覆盖" : "内置默认"),
    ),
  );
  save.type = "submit";
  uiText(save, "保存");
  reset.type = "button";
  uiText(reset, "恢复继承");
  reset.disabled = setting.source !== "user";
  form.append(label, source, save, reset);
  form.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      if (!options.current()) return;
      try {
        options.save(validateSettingsChange({ field, value: select.value }));
      } catch {
        select.setCustomValidity("请选择支持的值。");
        select.reportValidity();
      }
    },
    { signal: options.signal },
  );
  select.addEventListener("change", () => select.setCustomValidity(""), {
    signal: options.signal,
  });
  reset.addEventListener(
    "click",
    () => {
      if (options.current())
        options.save(validateSettingsChange({ field: `reset_${field}` }));
    },
    { signal: options.signal },
  );
  return form;
}
