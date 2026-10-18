import { invoke as nativeInvoke } from "@tauri-apps/api/core";
import {
  uiAttribute,
  message as uiMessage,
  uiText,
} from "./workspace-language";
import { terminalPresetLabels } from "./workspace-terminal-preset";
export type BehaviorApi = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;

export type BehaviorTarget = Readonly<{ kind: "group" | "host"; id: string }>;
const choices = {
  terminal_preset: Object.entries(terminalPresetLabels),
  term_type: [
    ["xterm-256color", "xterm-256color"],
    ["xterm", "xterm"],
    ["vt100", "vt100"],
  ],
  mac_option_as_meta: [
    ["false", "停用"],
    ["true", "启用"],
  ],
  backspace_send: [
    ["del", "DEL（^?）"],
    ["bs", "BS（^H）"],
  ],
  delete_send: [
    ["csi", "CSI（ESC [ 3 ~）"],
    ["del", "DEL（^?）"],
  ],
} as const;
type Key = keyof typeof choices;
const keys = Object.keys(choices) as Key[];
const labels: Record<Key, string> = {
  terminal_preset: "终端配色",
  term_type: "TERM",
  mac_option_as_meta: "macOS Option 作为 Meta",
  backspace_send: "Backspace 发送",
  delete_send: "Delete 发送",
};
export type BehaviorValues = Record<Key, string | boolean | null>;
export interface BehaviorView {
  token: string;
  overrides: BehaviorValues;
  values: Record<Key, { value: string | boolean; source: string }>;
}
function record(value: unknown): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("invalid behavior view");
  return value as Record<string, unknown>;
}
function exact(
  value: Record<string, unknown>,
  fields: readonly string[],
): void {
  if (Object.keys(value).sort().join(",") !== [...fields].sort().join(","))
    throw new Error("unexpected behavior fields");
}
function valid(key: Key, value: unknown): boolean {
  return key === "mac_option_as_meta"
    ? typeof value === "boolean"
    : typeof value === "string" && choices[key].some(([v]) => v === value);
}
export function decodeBehaviorView(value: unknown): BehaviorView {
  const root = record(value);
  exact(root, ["version", "token", "data"]);
  if (
    root.version !== 2 ||
    typeof root.token !== "string" ||
    !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
      root.token,
    )
  )
    throw new Error("invalid behavior receipt");
  const data = record(root.data);
  exact(data, ["overrides", "values"]);
  const own = record(data.overrides);
  const values = record(data.values);
  exact(own, keys);
  exact(values, keys);
  const overrides = {} as BehaviorValues;
  const result = {} as BehaviorView["values"];
  for (const key of keys) {
    if (own[key] !== null && !valid(key, own[key]))
      throw new Error("invalid behavior override");
    const row = record(values[key]);
    exact(row, ["value", "source"]);
    if (
      !valid(key, row.value) ||
      typeof row.source !== "string" ||
      !["default", "user", "group", "host"].includes(row.source)
    )
      throw new Error("invalid behavior value");
    overrides[key] = own[key] as string | boolean | null;
    result[key] = {
      value: row.value as string | boolean,
      source: row.source as string,
    };
  }
  return { token: root.token, overrides, values: result };
}
function describe(key: Key, value: string | boolean): string {
  return choices[key].find(([v]) => v === String(value))?.[1] ?? String(value);
}
const sources: Record<string, string> = {
  default: "默认",
  user: "全局",
  group: "直属分组",
  host: "SSH 入口",
};
export class OrganizationBehaviorEditor {
  private view?: BehaviorView;
  private busy = false;
  private invalid = false;
  private generation = 0;
  private readonly nonce: string | undefined;
  private readonly controls = new Map<Key, HTMLSelectElement>();
  private readonly status = document.createElement("p");
  private readonly actions = document.createElement("div");
  constructor(
    private readonly options: {
      root: HTMLElement;
      invoke?: BehaviorApi;
      nonce: () => string | undefined;
      target: BehaviorTarget;
      name: string;
      signal: AbortSignal;
      working: (busy: boolean) => void;
      back: () => void;
    },
  ) {
    this.nonce = options.nonce();
    this.actions.className = "terminal-paste-actions";
    options.signal.addEventListener(
      "abort",
      () => {
        ++this.generation;
        this.view = undefined;
        this.options.working(false);
      },
      { once: true },
    );
    void this.load();
  }
  private async load(values?: BehaviorValues): Promise<void> {
    if (
      this.busy ||
      this.options.signal.aborted ||
      !this.nonce ||
      this.options.nonce() !== this.nonce ||
      (values && (!this.view || this.invalid))
    )
      return;
    this.busy = true;
    this.options.working(true);
    const generation = ++this.generation;
    this.disable(true);
    uiText(this.status, values ? "正在保存终端行为…" : "正在读取终端行为…");
    try {
      const raw = await (this.options.invoke ?? nativeInvoke)(
        "workspace_organization_behavior",
        {
          nonce: this.nonce,
          target: { ...this.options.target },
          change: values ? { token: this.view?.token, values } : null,
        },
      );
      if (generation !== this.generation || this.options.signal.aborted) return;
      if (this.options.nonce() !== this.nonce) throw new Error("page changed");
      this.view = decodeBehaviorView(raw);
      this.invalid = false;
      this.render();
      uiText(
        this.status,
        values
          ? "已保存。新建 SSH 会话采用此配置；现有会话保持原值。"
          : "修改后统一保存；选择继承会移除此层覆盖。",
      );
    } catch {
      if (generation !== this.generation || this.options.signal.aborted) return;
      this.view = undefined;
      this.invalid = true;
      if (!this.actions.isConnected) this.render();
      uiText(
        this.status,
        "读取或保存未确认，可能已发生源变更。请重新读取后再编辑。",
      );
    } finally {
      if (generation === this.generation && !this.options.signal.aborted) {
        this.busy = false;
        this.options.working(false);
        this.disable(false);
      }
    }
  }
  private disable(busy: boolean): void {
    for (const element of this.options.root.querySelectorAll<
      HTMLButtonElement | HTMLSelectElement
    >("button,select")) {
      element.disabled =
        busy || (this.invalid && element.dataset.requiresView === "true");
    }
  }
  private render(): void {
    const { root, target, signal } = this.options;
    root.replaceChildren();
    this.controls.clear();
    const heading = document.createElement("h3");
    uiText(
      heading,
      uiMessage(
        "{0}终端行为 · {1}",
        uiMessage(target.kind === "host" ? "SSH 入口" : "分组"),
        this.options.name,
      ),
    );
    const note = document.createElement("p");
    uiText(
      note,
      target.kind === "host"
        ? "优先级：此入口 → 直属分组 → 全局 → 默认。仅影响此入口新建的 SSH 会话。"
        : "此分组的直接成员继承这些行为；不叠加祖先分组，不影响本地 Profile。",
    );
    root.append(heading, note);
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    for (const key of keys) {
      const label = document.createElement("label");
      uiText(label, labels[key]);
      const select = document.createElement("select");
      uiAttribute(select, "aria-label", labels[key]);
      select.dataset.behavior = key;
      select.dataset.requiresView = "true";
      const inherit = document.createElement("option");
      inherit.value = "";
      uiText(inherit, "继承");
      select.append(inherit);
      for (const [value, text] of choices[key]) {
        const option = document.createElement("option");
        option.value = value;
        uiText(option, text);
        select.append(option);
      }
      const own = this.view?.overrides[key];
      select.value = own === null || own === undefined ? "" : String(own);
      label.append(select);
      const hint = document.createElement("small");
      hint.id = `organization-behavior-${target.kind}-${target.id}-${key}-source`;
      select.setAttribute("aria-describedby", hint.id);
      const row = this.view?.values[key];
      uiText(
        hint,
        row
          ? uiMessage(
              "当前：{0}（{1}）",
              uiMessage(describe(key, row.value)),
              uiMessage(sources[row.source] ?? "默认"),
            )
          : "尚未读取",
      );
      label.append(hint);
      form.append(label);
      this.controls.set(key, select);
    }
    form.addEventListener(
      "submit",
      (event) => {
        event.preventDefault();
        if (this.busy || !this.view || this.invalid || signal.aborted) return;
        const values = {} as BehaviorValues;
        for (const key of keys) {
          const value = this.controls.get(key)?.value ?? "";
          values[key] =
            value === ""
              ? null
              : key === "mac_option_as_meta"
                ? value === "true"
                : value;
        }
        void this.load(values);
      },
      { signal },
    );
    this.actions.replaceChildren();
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, "保存终端行为");
    save.dataset.requiresView = "true";
    this.actions.append(save);
    const reload = document.createElement("button");
    reload.type = "button";
    uiText(reload, "重新读取");
    reload.addEventListener("click", () => void this.load(), { signal });
    this.actions.append(reload);
    const back = document.createElement("button");
    back.type = "button";
    uiText(back, "返回组织管理");
    back.addEventListener("click", this.options.back, { signal });
    this.actions.append(back);
    form.append(this.actions);
    this.status.setAttribute("role", "status");
    root.append(form, this.status);
  }
}
