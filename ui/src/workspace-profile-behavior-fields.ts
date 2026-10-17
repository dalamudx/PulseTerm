import { uiAttribute, uiText } from "./workspace-language";
import type { ProfileAppearance } from "./workspace-profile-editor";
import {
  isTerminalPreset,
  terminalPresetLabels,
} from "./workspace-terminal-preset";
/** Profile overrides remain sparse and use the existing single-save ticket. */
export function profileBehaviorFields(
  doc: Document,
  appearance: ProfileAppearance,
) {
  const root = doc.createElement("fieldset"),
    legend = doc.createElement("legend");
  uiText(legend, "终端配色与按键（新终端生效）");
  root.append(legend);
  const choice = (
    caption: string,
    value: string | null | undefined,
    choices: readonly (readonly [string, string])[],
  ) => {
    const label = doc.createElement("label"),
      select = doc.createElement("select");
    uiText(label, caption);
    uiAttribute(select, "aria-label", caption);
    for (const [key, text] of [["inherit", "继承全局设置"], ...choices]) {
      const item = doc.createElement("option");
      item.value = key ?? "";
      uiText(item, text ?? "");
      select.append(item);
    }
    select.value = value ?? "inherit";
    label.append(select);
    root.append(label);
    return select;
  };
  const preset = choice(
    "终端配色",
    appearance.colorPreset,
    Object.entries(terminalPresetLabels),
  );
  const meta = choice(
    "macOS Option 作为 Meta",
    appearance.macOptionIsMeta == null
      ? null
      : appearance.macOptionIsMeta
        ? "on"
        : "off",
    [
      ["on", "启用"],
      ["off", "停用"],
    ],
  );
  const backspace = choice("Backspace 发送编码", appearance.backspaceSend, [
    ["del", "^?（DEL）"],
    ["bs", "^H（BS）"],
  ]);
  const del = choice("Delete 发送编码", appearance.deleteSend, [
    ["csi", "ESC [ 3 ~"],
    ["del", "^?（DEL）"],
  ]);
  return {
    root,
    read() {
      if (
        (preset.value !== "inherit" && !isTerminalPreset(preset.value)) ||
        !["inherit", "on", "off"].includes(meta.value) ||
        !["inherit", "del", "bs"].includes(backspace.value) ||
        !["inherit", "csi", "del"].includes(del.value)
      )
        throw Error("profile.invalid");
      return {
        colorPreset:
          preset.value === "inherit"
            ? null
            : (preset.value as import("./workspace-terminal-preset").TerminalPreset),
        macOptionIsMeta: meta.value === "inherit" ? null : meta.value === "on",
        backspaceSend:
          backspace.value === "inherit"
            ? null
            : (backspace.value as "del" | "bs"),
        deleteSend:
          del.value === "inherit" ? null : (del.value as "csi" | "del"),
      };
    },
  };
}
