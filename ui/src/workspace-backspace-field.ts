import { message, uiAttribute, uiText } from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";

export function backspaceField(options: {
  document: Document;
  setting: DisplaySetting<"del" | "bs">;
  current: () => boolean;
  save: (change: SettingsChange) => void;
}): HTMLFormElement {
  const doc = options.document;
  const form = doc.createElement("form");
  const label = doc.createElement("label");
  uiText(label, "Backspace 发送");
  const select = doc.createElement("select");
  uiAttribute(select, "aria-label", "Backspace 发送");
  uiAttribute(
    select,
    "title",
    "仅普通 Backspace；组合输入和修饰键保持原行为。下次打开或重建终端视图时生效。",
  );
  for (const [value, caption] of [
    ["del", "DEL（^?，默认）"],
    ["bs", "BS（^H）"],
  ] as const) {
    const option = doc.createElement("option");
    option.value = value;
    uiText(option, caption);
    select.append(option);
  }
  select.value = options.setting.value;
  label.append(select);
  const source = doc.createElement("span");
  uiText(
    source,
    message(
      "来源：{0}",
      message(options.setting.source === "user" ? "用户覆盖" : "内置默认"),
    ),
  );
  const save = doc.createElement("button");
  save.type = "submit";
  uiText(save, "保存");
  uiAttribute(
    save,
    "aria-label",
    message("保存{0}", message("Backspace 发送")),
  );
  const reset = doc.createElement("button");
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiAttribute(
    reset,
    "aria-label",
    message("{0}恢复继承", message("Backspace 发送")),
  );
  form.addEventListener("submit", (event) => {
    event.preventDefault();
    if (
      options.current() &&
      (select.value === "del" || select.value === "bs")
    ) {
      options.save({ field: "backspace_send", value: select.value });
    }
  });
  reset.addEventListener("click", () => {
    if (options.current()) options.save({ field: "reset_backspace_send" });
  });
  form.append(label, source, save, reset);
  return form;
}
