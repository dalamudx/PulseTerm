import license from "./assets/fonts/jetbrains-mono/OFL.txt?raw";
import { TERMINAL_FONT_PRESETS } from "./features/terminal/font-presets";
import { uiAttribute, uiText } from "./workspace-language";

/** Selection edits one field only; the existing form's Save authorizes persistence. */
export function bindFontPreset(
  doc: Document,
  label: HTMLLabelElement,
  input: HTMLInputElement,
  row: HTMLFormElement,
  current: () => boolean,
): void {
  const select = doc.createElement("select");
  select.className = "workspace-font-preset";
  uiAttribute(select, "aria-label", "字体预设");
  for (const preset of TERMINAL_FONT_PRESETS) {
    const option = doc.createElement("option");
    option.value = preset.id;
    uiText(option, preset.label);
    select.append(option);
  }
  const custom = doc.createElement("option");
  custom.value = "custom";
  uiText(custom, "自定义字体");
  select.append(custom);
  select.value =
    TERMINAL_FONT_PRESETS.find((preset) => preset.family === input.value)?.id ??
    "custom";
  input.hidden = select.value !== "custom";
  select.addEventListener("change", () => {
    if (!current()) return;
    const preset = TERMINAL_FONT_PRESETS.find(
      (value) => value.id === select.value,
    );
    if (preset) input.value = preset.family;
    input.hidden = !!preset;
  });
  label.append(select);
  const notice = doc.createElement("span");
  notice.className = "workspace-font-license";
  const toggle = doc.createElement("button");
  toggle.type = "button";
  uiText(toggle, "许可");
  uiAttribute(toggle, "aria-label", "JetBrains Mono 字体许可");
  toggle.ariaExpanded = "false";
  const text = doc.createElement("pre");
  uiText(text, license);
  text.hidden = true;
  toggle.addEventListener("click", () => {
    if (!current()) return;
    text.hidden = !text.hidden;
    toggle.ariaExpanded = String(!text.hidden);
    notice.className = text.hidden
      ? "workspace-font-license"
      : "workspace-font-license is-open";
  });
  notice.append(toggle, text);
  row.append(notice);
}
