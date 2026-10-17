import { CatalogueBatch } from "./workspace-catalogue-batch";
import {
  type IdentityCatalogue,
  type IdentityChange,
  type IdentityFields,
  readIdentities,
  saveIdentity,
  validateIdentityChange,
} from "./workspace-identities";
import { saveIdentityPassword } from "./workspace-identity-password";
import { uiAttribute, uiText } from "./workspace-language";
export class WorkspaceIdentityEditor {
  private batch: CatalogueBatch | undefined;
  private readonly passwordInputs = new Set<HTMLInputElement>();
  private clearPasswords(): void {
    for (const input of this.passwordInputs) {
      input.value = "";
      input.type = "password";
    }
    this.passwordInputs.clear();
  }
  private value: IdentityCatalogue | undefined;
  private epoch = 0;
  private renderGeneration = 0;
  private busy = false;
  private disposed = false;
  private operation: AbortController | undefined;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      done: HTMLButtonElement;
      fields: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
    },
    private readonly dependencies = {
      read: readIdentities,
      save: saveIdentity,
    },
    private readonly writePassword = saveIdentityPassword,
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
  }
  async open(): Promise<void> {
    if (this.disposed || this.busy || this.options.dialog.open) return;
    this.options.dialog.showModal();
    await this.load();
  }
  close(): void {
    this.clearPasswords();
    this.epoch++;
    this.operation?.abort();
    this.operation = undefined;
    this.value = undefined;
    this.busy = false;
    this.options.fields.replaceChildren();
    this.options.working(false);
    if (this.options.dialog.open) this.options.dialog.close();
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      epoch === this.epoch
    );
  }
  private lock(): void {
    for (const control of this.options.fields.querySelectorAll<
      HTMLInputElement | HTMLButtonElement | HTMLSelectElement
    >("input,button,select"))
      control.disabled = this.busy;
    this.batch?.setBusy(this.busy);
  }
  private async load(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce) {
      uiText(this.options.status, "页面尚未确认 · 请稍后重新打开");
      return;
    }
    const epoch = ++this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.fields.replaceChildren();
    uiText(this.options.status, "正在读取身份…");
    try {
      const value = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.value = value;
      this.render();
      uiText(
        this.options.status,
        value.vaultPresent
          ? "身份与密码保存 · 密码存在不代表已解锁或允许连接"
          : "尚未创建凭据库 · 当前不能添加身份，不会自动生成密钥",
      );
    } catch {
      if (this.current(epoch))
        uiText(
          this.options.status,
          "无法确认身份来源 · 不显示默认身份，关闭后重新打开",
        );
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
      }
    }
  }
  private render(selected?: string): void {
    const value = this.value;
    if (!value) return;
    this.clearPasswords();
    this.options.fields.replaceChildren();
    const generation = ++this.renderGeneration;
    const epoch = this.epoch;
    if (!value.vaultPresent) {
      const empty = document.createElement("p");
      uiText(empty, "还没有可用的凭据库。身份添加功能暂不可用。");
      this.options.fields.append(empty);
      return;
    }
    const label = document.createElement("label");
    uiText(label, "编辑身份 ");
    const select = document.createElement("select");
    uiAttribute(select, "aria-label", "编辑身份");
    const create = document.createElement("option");
    create.value = "";
    uiText(create, "添加身份");
    select.append(create);
    for (const identity of value.identities) {
      const option = document.createElement("option");
      option.value = identity.id;
      option.textContent = identity.label;
      select.append(option);
    }
    select.value = selected ?? value.identities[0]?.id ?? "";
    label.append(select);
    this.options.fields.append(label);
    const valid = () =>
      this.current(epoch) &&
      this.value === value &&
      !this.busy &&
      generation === this.renderGeneration;
    select.addEventListener("change", () => {
      if (valid()) {
        this.render(select.value);
        this.lock();
      }
    });
    const row = value.identities.find(
      (identity) => identity.id === select.value,
    );
    this.form(
      row?.id,
      row ?? { label: "", username: "" },
      row?.secret.hasValue ?? false,
      value,
      generation,
      epoch,
    );
    this.batch = new CatalogueBatch({
      container: this.options.fields,
      rows: value.identities.map((row) => ({
        id: row.id,
        label: row.label,
        description: row.username,
      })),
      kind: "密码身份",
      consequence: "名称和引用会被删除，已保存的凭据保留。现有会话会继续运行。",
      current: () =>
        this.current(epoch) &&
        this.value === value &&
        !this.busy &&
        this.renderGeneration === generation,
      edit: (id) => {
        this.render(id);
        this.lock();
      },
      filter: () => this.clearPasswords(),
      remove: (ids) => {
        void this.save({ action: "remove_many", ids });
      },
    });
  }
  private form(
    id: string | undefined,
    initial: IdentityFields,
    hasValue: boolean,
    value: IdentityCatalogue,
    generation: number,
    epoch: number,
  ): void {
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const inputs = new Map<keyof IdentityFields, HTMLInputElement>();
    for (const [key, caption, max] of [
      ["label", "显示名称", 256],
      ["username", "用户名", 255],
    ] as const) {
      const label = document.createElement("label");
      uiText(label, caption);
      const input = document.createElement("input");
      input.type = "text";
      input.value = initial[key];
      input.required = true;
      input.maxLength = max;
      input.autocomplete = "off";
      input.spellcheck = false;
      label.append(input);
      form.append(label);
      inputs.set(key, input);
    }
    const state = document.createElement("p");
    uiText(
      state,
      hasValue
        ? "密码状态：已保存 · 不代表已解锁或允许使用"
        : "密码状态：未设置 · 先保存身份，再单独保存密码",
    );
    form.append(state);
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, id ? "保存" : "添加");
    actions.append(save);
    const valid = () =>
      this.current(epoch) &&
      this.value === value &&
      !this.busy &&
      generation === this.renderGeneration;
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!valid()) return;
      const fields = {
        label: inputs.get("label")?.value ?? "",
        username: inputs.get("username")?.value ?? "",
      };
      const change: IdentityChange = id
        ? { action: "update", id, ...fields }
        : { action: "create", ...fields };
      try {
        validateIdentityChange(change);
        void this.save(change);
      } catch {
        uiText(
          this.options.status,
          "请检查名称和用户名；不允许控制字符、隐形方向字符或用户名选项前缀",
        );
      }
    });
    if (id) {
      const remove = document.createElement("button");
      remove.type = "button";
      uiText(remove, "删除");
      let confirmed = false;
      remove.addEventListener("click", () => {
        if (!valid()) return;
        if (!confirmed) {
          confirmed = true;
          uiText(remove, "确认删除身份");
          return;
        }
        void this.save({ action: "remove", id });
      });
      actions.append(remove);
    }
    form.append(actions);
    this.options.fields.append(form);
    if (id)
      this.passwordForm(id, valid, () =>
        Array.from(inputs).every(
          ([key, input]) => input.value === initial[key],
        ),
      );
  }
  private passwordForm(
    id: string,
    valid: () => boolean,
    metadataSaved: () => boolean,
  ): void {
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const label = document.createElement("label");
    uiText(label, "保存新的密码 · 需先解锁凭据库");
    const input = document.createElement("input");
    input.type = "password";
    input.required = true;
    input.maxLength = 4096;
    input.autocomplete = "new-password";
    input.spellcheck = false;
    uiAttribute(input, "aria-label", "新的身份密码");
    label.append(input);
    form.append(label);
    this.passwordInputs.add(input);
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const show = document.createElement("button");
    show.type = "button";
    uiText(show, "显示输入");
    show.setAttribute("aria-pressed", "false");
    show.addEventListener("click", () => {
      if (!valid()) return;
      const visible = input.type === "password";
      input.type = visible ? "text" : "password";
      uiText(show, visible ? "隐藏输入" : "显示输入");
      show.setAttribute("aria-pressed", String(visible));
      input.focus();
    });
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, "保存密码");
    actions.append(show, save);
    form.append(actions);
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!valid()) return;
      if (!metadataSaved()) {
        input.value = "";
        uiText(this.options.status, "请先保存名称和用户名，再单独保存密码");
        return;
      }
      const bytes = new TextEncoder().encode(input.value);
      input.value = "";
      void this.savePassword(id, bytes);
    });
    this.options.fields.append(form);
  }
  private async savePassword(id: string, bytes: Uint8Array): Promise<void> {
    const nonce = this.options.nonce(),
      value = this.value,
      epoch = this.epoch;
    if (!nonce || !value || !this.current(epoch) || this.busy) {
      bytes.fill(0);
      return;
    }
    this.clearPasswords();
    const operation = new AbortController();
    this.operation?.abort();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.lock();
    uiText(this.options.status, "正在确认密码保存…");
    try {
      await this.writePassword(nonce, id, bytes, operation.signal);
      if (!this.current(epoch)) return;
      const next = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.value = next;
      this.render(id);
      this.options.done.focus();
      uiText(
        this.options.status,
        next.identities.find((row) => row.id === id)?.secret.hasValue
          ? "密码已保存 · 未启动连接，也不授予凭据使用权限"
          : "写入已确认 · 当前身份或密码状态已变化，请重新检查",
      );
    } catch {
      if (this.current(epoch)) {
        this.value = undefined;
        this.clearPasswords();
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "密码保存未确认 · 请先解锁凭据库；若已解锁，关闭后重开检查实际结果，不自动重试",
        );
        this.options.done.focus();
      }
    } finally {
      bytes.fill(0);
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
      }
    }
  }

  private async save(change: IdentityChange): Promise<void> {
    const value = this.value,
      nonce = this.options.nonce();
    if (!value || !nonce || this.busy || this.disposed) return;
    const epoch = this.epoch;
    this.clearPasswords();
    const operation = new AbortController();
    this.operation?.abort();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.lock();
    uiText(this.options.status, "正在保存身份…");
    try {
      const next = await this.dependencies.save(
        nonce,
        value.token,
        change,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.value = next;
      const selected =
        change.action === "create"
          ? next.identities.find(
              (identity) =>
                !value.identities.some((old) => old.id === identity.id),
            )?.id
          : change.action === "update"
            ? change.id
            : next.identities[0]?.id;
      this.render(selected ?? "");
      this.options.done.focus();
      uiText(
        this.options.status,
        "已保存 · 未改变凭据库锁定状态或启动连接，本地终端不受影响",
      );
    } catch {
      if (this.current(epoch)) {
        this.value = undefined;
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "保存未确认或版本冲突 · 不自动重试，关闭后重新打开以读取实际结果",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
      }
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.close();
    this.disposed = true;
    this.options.signal.removeEventListener("abort", this.abort);
  }
}
