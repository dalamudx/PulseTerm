import { uiAttribute, uiText } from "./workspace-language";
import type { DisplaySetting, SettingsChange } from "./workspace-settings";

export type UiAppearance = "system" | "light" | "dark";
export function isUiAppearance(value: unknown): value is UiAppearance {
  return value === "system" || value === "light" || value === "dark";
}
const query = "(prefers-color-scheme: dark)";
function systemQuery(document: Document): MediaQueryList | undefined {
  try {
    return document.defaultView?.matchMedia?.(query);
  } catch {
    return undefined;
  }
}
function resolved(mode: UiAppearance, media: MediaQueryList | undefined) {
  if (mode !== "system") return mode;
  if (typeof media?.matches !== "boolean") return undefined;
  return media.matches ? "dark" : "light";
}
/** Fixed UI classes only. This never touches a terminal or grants theme-apply authority. */
export class WorkspaceAppearance {
  private mode: UiAppearance | undefined;
  private readonly media: MediaQueryList | undefined;
  private readonly change = () => this.render();
  constructor(
    private readonly document: Document,
    private readonly signal: AbortSignal,
  ) {
    this.media = systemQuery(document);
    if (!signal.aborted) {
      this.media?.addEventListener("change", this.change);
      signal.addEventListener(
        "abort",
        () => this.media?.removeEventListener("change", this.change),
        {
          once: true,
        },
      );
    }
  }
  apply(mode: UiAppearance): boolean {
    if (this.signal.aborted || !isUiAppearance(mode)) return false;
    this.mode = mode;
    return this.render();
  }
  private render(): boolean {
    if (this.signal.aborted || !this.mode) return false;
    const appearance = resolved(this.mode, this.media);
    if (!appearance) return false;
    this.document.documentElement.classList.toggle(
      "dark",
      appearance === "dark",
    );
    this.document.documentElement.classList.toggle(
      "light",
      appearance === "light",
    );
    return true;
  }
}

/** A draft changes this local preview only. Saving still uses the original settings ticket. */
export function uiAppearanceField(options: {
  document: Document;
  setting: DisplaySetting<UiAppearance>;
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
    heading = doc.createElement("strong"),
    sample = doc.createElement("p"),
    note = doc.createElement("p"),
    media = systemQuery(doc);
  row.className = "workspace-setting-row workspace-appearance-row";
  uiText(label, "界面外观");
  uiAttribute(select, "aria-label", "界面外观");
  for (const [value, text] of [
    ["light", "浅色"],
    ["dark", "深色"],
    ["system", "跟随系统"],
  ] as const) {
    const option = doc.createElement("option");
    option.value = value;
    uiText(option, text);
    select.append(option);
  }
  select.value = options.setting.value;
  label.append(select);
  uiText(source, options.setting.source === "user" ? "用户覆盖" : "内置默认");
  save.type = "submit";
  uiText(save, "保存");
  uiAttribute(save, "aria-label", "保存界面外观");
  reset.type = "button";
  uiText(reset, "恢复继承");
  uiAttribute(reset, "aria-label", "界面外观恢复继承");
  uiAttribute(preview, "aria-label", "界面外观预览");
  uiText(heading, "PulseTerm");
  uiText(note, "仅预览 · 保存后立即应用界面外观");
  note.className = "workspace-appearance-note";
  preview.append(heading, sample, note);
  const update = () => {
    if (signal.aborted || !isUiAppearance(select.value)) return;
    const appearance = resolved(select.value, media);
    preview.className = `workspace-appearance-preview ${appearance ?? "light"}`;
    uiText(
      sample,
      appearance === "dark"
        ? "深色界面 · 主机与会话"
        : appearance === "light"
          ? "浅色界面 · 主机与会话"
          : "系统外观暂不可用",
    );
  };
  select.addEventListener("change", update, { signal });
  media?.addEventListener("change", update);
  signal.addEventListener(
    "abort",
    () => media?.removeEventListener("change", update),
    { once: true },
  );
  row.addEventListener(
    "submit",
    (event) => {
      event.preventDefault();
      if (!signal.aborted && options.current() && isUiAppearance(select.value))
        options.save({ field: "ui_appearance", value: select.value });
    },
    { signal },
  );
  reset.addEventListener(
    "click",
    () => {
      if (!signal.aborted && options.current())
        options.save({ field: "reset_ui_appearance" });
    },
    { signal },
  );
  row.append(label, source, save, reset, preview);
  update();
  return row;
}
