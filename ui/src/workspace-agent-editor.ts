import {
  type AgentCatalogue,
  type AgentChange,
  cancelAgentCatalogue,
  readAgentCatalogue,
  saveAgentCatalogue,
} from "./workspace-agent-identities";
import { message, uiAttribute, uiText } from "./workspace-language";
export class WorkspaceAgentEditor {
  private epoch = 0;
  private original: string | undefined;
  private controller: AbortController | undefined;
  private catalogue: AgentCatalogue | undefined;
  private focus: HTMLElement | undefined;
  constructor(
    private readonly o: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      status: HTMLElement;
      done: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(value: boolean): void;
    },
    private readonly deps = {
      read: readAgentCatalogue,
      save: saveAgentCatalogue,
      cancel: cancelAgentCatalogue,
    },
  ) {
    o.done.addEventListener("click", () => this.close());
    o.dialog.addEventListener("cancel", (e) => {
      e.preventDefault();
      this.close();
    });
    o.signal.addEventListener("abort", () => this.close(), { once: true });
  }
  private current(epoch: number): boolean {
    return (
      epoch === this.epoch &&
      !this.o.signal.aborted &&
      !this.controller?.signal.aborted &&
      this.original === this.o.nonce() &&
      this.o.dialog.open
    );
  }
  async open(): Promise<void> {
    if (this.o.signal.aborted || this.o.dialog.open) return;
    const nonce = this.o.nonce();
    if (!nonce) return;
    this.original = nonce;
    const el = this.o.dialog.ownerDocument.activeElement;
    this.focus = el instanceof HTMLElement ? el : undefined;
    this.controller = new AbortController();
    const epoch = ++this.epoch;
    this.o.fields.replaceChildren();
    this.o.dialog.showModal();
    this.o.done.focus();
    await this.load(epoch, false);
  }
  private async load(epoch: number, discover: boolean): Promise<void> {
    const nonce = this.original,
      signal = this.controller?.signal;
    if (!nonce || !signal) return;
    this.o.working(true);
    this.o.fields.replaceChildren();
    uiText(
      this.o.status,
      discover ? "正在读取本机 Agent 的公钥…" : "正在读取保存的 Agent 身份…",
    );
    try {
      const c = await this.deps.read(nonce, signal, discover);
      if (!this.current(epoch)) {
        void this.deps.cancel(nonce, c.token);
        return;
      }
      this.catalogue = c;
      this.render();
      uiText(
        this.o.status,
        discover
          ? c.keys.length
            ? "选择一把公钥，填写名称和登录用户名后保存。"
            : "本机 Agent 未提供可用公钥，请添加密钥后重新读取。"
          : "保存身份后可在连接表单中明确选择它。",
      );
    } catch {
      if (this.current(epoch))
        uiText(
          this.o.status,
          "无法读取 Agent 身份。请检查本机 Agent，关闭后重试。",
        );
    } finally {
      if (this.current(epoch)) this.o.working(false);
    }
  }
  private render(): void {
    const c = this.catalogue;
    if (!c) return;
    const doc = this.o.fields.ownerDocument;
    this.o.fields.replaceChildren();
    const button = (text: string, action: () => void) => {
      const b = doc.createElement("button");
      b.type = "button";
      uiText(b, text);
      b.addEventListener("click", action);
      return b;
    };
    const fields = (form: HTMLFormElement, label: string, user: string) => {
      const name = doc.createElement("input"),
        username = doc.createElement("input");
      name.value = label;
      name.maxLength = 256;
      name.required = true;
      username.value = user;
      username.maxLength = 256;
      username.required = true;
      for (const [caption, input] of [
        ["身份名称", name],
        ["登录用户名", username],
      ] as const) {
        const l = doc.createElement("label");
        uiText(l, caption);
        uiAttribute(input, "aria-label", caption);
        input.autocomplete = "off";
        l.append(input);
        form.append(l);
      }
      return { name, username };
    };
    this.o.fields.append(
      button("读取本机 Agent 公钥", () => void this.load(this.epoch, true)),
    );
    if (c.keys.length) {
      const form = doc.createElement("form");
      form.className = "workspace-host-form";
      const l = doc.createElement("label"),
        select = doc.createElement("select");
      uiText(l, "Agent 公钥");
      uiAttribute(select, "aria-label", "选择 Agent 公钥");
      for (const [k, i] of c.keys.map((k, i) => [k, i] as const)) {
        const option = doc.createElement("option");
        option.value = String(i);
        option.textContent = `${k.algorithm} ${k.fingerprint}`;
        select.append(option);
      }
      l.append(select);
      form.append(l);
      const f = fields(form, "", "");
      const save = doc.createElement("button");
      save.type = "submit";
      uiText(save, "保存所选 Agent 身份");
      form.append(save);
      form.addEventListener("submit", (e) => {
        e.preventDefault();
        void this.save({
          action: "create",
          key: Number(select.value),
          label: f.name.value,
          username: f.username.value,
        });
      });
      this.o.fields.append(form);
    }
    for (const row of c.identities) {
      const form = doc.createElement("form");
      form.className = "workspace-host-form";
      const info = doc.createElement("p");
      info.textContent = `${row.algorithm} ${row.fingerprint}`;
      form.append(info);
      const f = fields(form, row.label, row.username);
      const save = doc.createElement("button");
      save.type = "submit";
      uiText(save, "保存名称与用户名");
      form.append(save);
      form.addEventListener("submit", (e) => {
        e.preventDefault();
        void this.save({
          action: "update",
          id: row.id,
          label: f.name.value,
          username: f.username.value,
        });
      });
      const remove = button("删除此 Agent 身份", () => {
        const confirm = doc.createElement("p");
        uiText(confirm, message("删除 {0}（{1}）？", row.label, row.username));
        const actions = doc.createElement("div");
        actions.className = "terminal-paste-actions";
        actions.append(
          button("取消删除", () => this.render()),
          button(
            "确认删除 Agent 身份",
            () => void this.save({ action: "remove", id: row.id }),
          ),
        );
        form.replaceChildren(confirm, actions);
      });
      form.append(remove);
      this.o.fields.append(form);
    }
    if (!c.identities.length && !c.keys.length) {
      const p = doc.createElement("p");
      uiText(p, "还没有保存的 Agent 身份。读取本机公钥后选择一把保存。");
      this.o.fields.append(p);
    }
  }
  private async save(change: AgentChange): Promise<void> {
    const c = this.catalogue,
      nonce = this.original,
      signal = this.controller?.signal,
      epoch = this.epoch;
    if (!c || !nonce || !signal || !this.current(epoch)) return;
    for (const e of this.o.fields.querySelectorAll<
      HTMLInputElement | HTMLSelectElement | HTMLButtonElement
    >("input,select,button"))
      e.disabled = true;
    this.o.working(true);
    uiText(this.o.status, "正在保存 Agent 身份…");
    try {
      const next = await this.deps.save(nonce, c, change, signal);
      if (!this.current(epoch)) {
        void this.deps.cancel(nonce, next.token);
        return;
      }
      this.catalogue = next;
      this.render();
      uiText(this.o.status, "Agent 身份已保存。");
    } catch {
      if (this.current(epoch)) {
        this.catalogue = undefined;
        this.o.fields.replaceChildren();
        uiText(this.o.status, "保存未确认，请关闭后重新读取；不要重复提交。");
      }
    } finally {
      if (this.current(epoch)) this.o.working(false);
    }
  }
  close(): void {
    const nonce = this.original,
      c = this.catalogue;
    const same = nonce !== undefined && nonce === this.o.nonce();
    ++this.epoch;
    this.controller?.abort();
    this.controller = undefined;
    this.catalogue = undefined;
    if (nonce && c) void this.deps.cancel(nonce, c.token);
    if (this.o.dialog.open) this.o.dialog.close();
    this.o.fields.replaceChildren();
    uiText(this.o.status, "");
    if (same && !this.o.signal.aborted) {
      this.o.working(false);
      if (this.focus?.isConnected) this.focus.focus();
    }
    this.original = undefined;
    this.focus = undefined;
  }
}
