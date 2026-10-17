import { formatChordSequence } from "./keybindings/chords";
import type { KeybindingAction } from "./keybindings/definitions";
import { workspaceKeybindingCommandIds } from "./workspace-command-catalog";
import {
  cancelWorkspaceKeymap,
  confirmWorkspaceKeymap,
  type KeymapChange,
  readWorkspaceKeymap,
  saveWorkspaceKeymap,
  validateKeymapChange,
  type WorkspaceKeymap,
} from "./workspace-keymap";
import { message, uiAttribute, uiNode, uiText } from "./workspace-language";

const titles: Readonly<Record<KeybindingAction, string>> = {
  profiles: "管理本地 Profile",
  settings: "终端设置",
  find: "查找当前终端",
  hosts: "管理 SSH 主机",
  identities: "管理 SSH 身份",
  vault: "保险库",
  ssh: "连接保存的 SSH 主机",
  "close-session": "关闭当前会话",
  palette: "命令面板",
};
export interface KeymapEditorOptions {
  readonly dialog: HTMLDialogElement;
  readonly fields: HTMLElement;
  readonly status: HTMLElement;
  readonly signal: AbortSignal;
  nonce(): string | undefined;
  working(value: boolean): void;
  invalidate(): void;
  changed(value: WorkspaceKeymap): void;
}
export class WorkspaceKeymapEditor {
  private projection: WorkspaceKeymap | undefined;
  private operation: AbortController | undefined;
  private nonce: string | undefined;
  private epoch = 0;
  private busy = false;
  private disposed = false;
  private selected: KeybindingAction = "palette";
  private opener: HTMLElement | null = null;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: KeymapEditorOptions,
    private readonly dependencies = {
      read: readWorkspaceKeymap,
      confirm: confirmWorkspaceKeymap,
      save: saveWorkspaceKeymap,
      cancel: cancelWorkspaceKeymap,
    },
  ) {
    options.signal.addEventListener("abort", this.abort, { once: true });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    if (options.signal.aborted) this.dispose();
  }
  async open(): Promise<void> {
    const nonce = this.options.nonce();
    if (this.disposed || this.busy || this.options.dialog.open || !nonce)
      return;
    this.nonce = nonce;
    this.opener = this.options.dialog.ownerDocument
      .activeElement as HTMLElement | null;
    this.options.dialog.showModal();
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.invalidate();
    this.options.fields.replaceChildren();
    uiText(this.options.status, "正在读取并确认当前页键位来源…");
    try {
      const candidate = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      const confirmed = await this.dependencies.confirm(
        nonce,
        candidate,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.projection = confirmed;
      this.options.changed(confirmed);
      this.render();
      uiText(
        this.options.status,
        "每个动作最多四个键位。空键位仅禁用快捷键，动作仍可点击。",
      );
    } catch {
      if (this.current(epoch))
        uiText(
          this.options.status,
          "键位来源不可用 · 未使用默认值覆盖文件。关闭后重新打开以读取。",
        );
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.unlock();
      }
    }
  }
  close(restore = true): void {
    const nonce = this.nonce,
      opener = this.opener,
      wasOpen = this.options.dialog.open;
    this.epoch++;
    const active = this.operation;
    active?.abort();
    this.operation = undefined;
    if (!active && nonce && wasOpen)
      void this.dependencies.cancel(nonce).catch(() => {});
    this.busy = false;
    this.projection = undefined;
    this.options.working(false);
    if (wasOpen) this.options.dialog.close();
    this.opener = null;
    this.nonce = undefined;
    if (
      restore &&
      !this.disposed &&
      !this.options.signal.aborted &&
      nonce === this.options.nonce() &&
      opener?.isConnected &&
      !this.options.dialog.ownerDocument.querySelector("dialog[open]")
    )
      opener.focus({ preventScroll: true });
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      epoch === this.epoch &&
      this.nonce === this.options.nonce()
    );
  }
  private unlock(): void {
    for (const control of this.options.fields.querySelectorAll<
      | HTMLInputElement
      | HTMLTextAreaElement
      | HTMLSelectElement
      | HTMLButtonElement
    >("input,textarea,select,button"))
      control.disabled = false;
    this.options.fields
      .querySelector<HTMLInputElement>("[data-rewrite]")
      ?.dispatchEvent(new Event("change"));
  }
  private render(): void {
    const value = this.projection;
    if (!value) return;
    const selected = this.selected;
    const doc = this.options.dialog.ownerDocument,
      form = doc.createElement("form");
    const live = () =>
      this.projection === value &&
      this.selected === selected &&
      form.isConnected &&
      this.current(this.epoch) &&
      !this.busy;
    form.className = "workspace-host-form";
    const select = doc.createElement("select"),
      actionLabel = doc.createElement("label");
    uiText(actionLabel, "动作");
    actionLabel.append(select);
    uiAttribute(select, "aria-label", "选择键位动作");
    for (const action of [
      ...workspaceKeybindingCommandIds,
      "palette",
    ] as const) {
      const option = doc.createElement("option");
      option.value = action;
      uiText(option, titles[action]);
      select.append(option);
    }
    select.value = this.selected;
    const source = value.values.sources.find(
        (source) => source.action === this.selected,
      ),
      bindings = value.values.bindings.filter(
        (binding) => binding.action === this.selected,
      );
    const provenance = doc.createElement("p");
    uiText(
      provenance,
      message(
        "来源：{0} · {1}",
        message(source?.source === "user" ? "用户覆盖" : "内置"),
        message(source?.disabled ? "无快捷键" : "已启用"),
      ),
    );
    const keys = doc.createElement("textarea"),
      keysLabel = doc.createElement("label");
    uiText(keysLabel, "键位（每行一个，最多四行）");
    keysLabel.append(keys);
    keys.rows = 4;
    keys.maxLength = 515;
    keys.spellcheck = false;
    keys.value = bindings
      .map((binding) => formatChordSequence(binding.sequence))
      .join("\n");
    keys.placeholder = "ctrl+shift+k ctrl+shift+s";
    const when = doc.createElement("input"),
      whenLabel = doc.createElement("label");
    uiText(whenLabel, "生效条件");
    whenLabel.append(when);
    when.type = "text";
    when.maxLength = 128;
    when.value =
      bindings[0]?.when
        .map((term) => `${term.required ? "" : "!"}${term.field}`)
        .join(" && ") ?? "workspaceFocus && !editableFocus && !modalOpen";
    const hint = doc.createElement("p");
    uiText(
      hint,
      "终端中仅接受 Ctrl/Meta+Shift+字母或数字。复制、粘贴、刷新键保留；条件仅支持 && 和 !。",
    );
    const warning = doc.createElement("p");
    uiText(
      warning,
      "保存会规范化 keymap.toml 的格式并移除注释。不会改变终端或 SSH 的执行权限。",
    );
    const consent = doc.createElement("input"),
      consentLabel = doc.createElement("label");
    consent.type = "checkbox";
    consentLabel.className = "workspace-keymap-consent";
    consent.dataset.rewrite = "true";
    consentLabel.append(consent, uiNode(doc, "我同意规范化此键位文件"));
    const save = doc.createElement("button"),
      disable = doc.createElement("button"),
      restore = doc.createElement("button");
    save.type = "submit";
    uiText(save, "保存此动作");
    disable.type = "button";
    uiText(disable, "禁用快捷键");
    restore.type = "button";
    uiText(restore, "恢复内置");
    const actions = doc.createElement("div");
    actions.className = "workspace-host-actions";
    actions.append(save, disable, restore);
    const ready = () => {
      for (const button of [save, disable, restore])
        button.disabled = this.busy || !consent.checked;
    };
    consent.addEventListener("change", ready);
    for (const input of [keys, when])
      input.addEventListener("input", () => {
        consent.checked = false;
        ready();
      });
    select.addEventListener("change", () => {
      if (!live()) return;
      const next = select.value;
      if (
        [...workspaceKeybindingCommandIds, "palette"].some(
          (action) => action === next,
        )
      ) {
        this.selected = next as KeybindingAction;
        this.render();
        this.options.fields.querySelector("select")?.focus();
      }
    });
    const set = (): KeymapChange =>
      validateKeymapChange({
        kind: "set",
        action: selected,
        keys: keys.value.trim()
          ? keys.value.split(/\r?\n/).map((key) => key.trim())
          : [],
        when: when.value,
      });
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!live()) return;
      try {
        void this.save(set(), consent.checked);
      } catch {
        uiText(
          this.options.status,
          "键位或条件无效／冲突。请检查固定修饰键和条件字段。",
        );
      }
    });
    disable.addEventListener("click", () => {
      if (!live()) return;
      try {
        void this.save(
          validateKeymapChange({
            kind: "set",
            action: selected,
            keys: [],
            when: when.value,
          }),
          consent.checked,
        );
      } catch {
        uiText(this.options.status, "条件无效，未发送保存。");
      }
    });
    restore.addEventListener("click", () => {
      if (live())
        void this.save({ kind: "restore", action: selected }, consent.checked);
    });
    form.append(
      actionLabel,
      provenance,
      keysLabel,
      whenLabel,
      hint,
      warning,
      consentLabel,
      actions,
    );
    this.options.fields.replaceChildren(form);
    ready();
  }
  private async save(change: KeymapChange, consent: boolean): Promise<void> {
    const value = this.projection,
      nonce = this.nonce;
    if (!value || !nonce || this.busy || !consent || !this.current(this.epoch))
      return;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.invalidate();
    for (const control of this.options.fields.querySelectorAll<
      | HTMLInputElement
      | HTMLTextAreaElement
      | HTMLSelectElement
      | HTMLButtonElement
    >("input,textarea,select,button"))
      control.disabled = true;
    uiText(this.options.status, "正在保存并确认新来源…");
    try {
      const saved = await this.dependencies.save(
        nonce,
        value,
        change,
        true,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      const confirmed = await this.dependencies.confirm(
        nonce,
        saved,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.projection = confirmed;
      this.options.changed(confirmed);
      this.render();
      uiText(this.options.status, "此动作键位已保存并确认。");
      this.options.fields.scrollTop = 0;
      this.options.dialog
        .querySelector<HTMLButtonElement>("#workspace-keymap-done")
        ?.focus({ preventScroll: true });
    } catch {
      if (this.current(epoch)) {
        this.projection = undefined;
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "保存未确认 · 请关闭后重新打开读取实际文件，不自动重试。",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.unlock();
      }
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.close(false);
    this.options.signal.removeEventListener("abort", this.abort);
  }
}
