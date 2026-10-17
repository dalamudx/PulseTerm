import type { IdentityFields } from "./workspace-identities";
import {
  type KeyIdentity,
  type KeyIdentityCatalogue,
  type KeyIdentityChange,
  readKeyIdentities,
  saveKeyIdentity,
  validateKeyIdentityChange,
} from "./workspace-key-identities";
import { message, uiAttribute, uiText } from "./workspace-language";
export interface KeyImportSupport {
  available(nonce: string, signal: AbortSignal): Promise<boolean>;
  save(
    nonce: string,
    identity: string,
    key: Uint8Array,
    passphrase: Uint8Array | undefined,
    signal: AbortSignal,
  ): Promise<void>;
}

import { CatalogueBatch } from "./workspace-catalogue-batch";
export class WorkspaceKeyIdentityEditor {
  private batch: CatalogueBatch | undefined;
  private value: KeyIdentityCatalogue | undefined;
  private epoch = 0;
  private renderGeneration = 0;
  private busy = false;
  private disposed = false;
  private pageNonce: string | undefined;
  private operation: AbortController | undefined;
  private readonly abort = () => this.dispose();
  private importEnabled = false;
  private keyInputs: Array<HTMLInputElement | HTMLTextAreaElement> = [];
  private refreshImport: (() => void) | undefined;
  private clearKeyInputs(): void {
    for (const input of this.keyInputs) input.value = "";
    this.keyInputs = [];
    this.refreshImport = undefined;
  }
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      done: HTMLButtonElement;
      fields: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
      imports?: KeyImportSupport;
    },
    private readonly dependencies = {
      read: readKeyIdentities,
      save: saveKeyIdentity,
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
  }
  async open(): Promise<void> {
    if (this.disposed || this.busy || this.options.dialog.open) return;
    this.options.dialog.showModal();
    await this.load();
  }
  close(): void {
    this.epoch++;
    this.operation?.abort();
    this.operation = undefined;
    this.clearKeyInputs();
    this.importEnabled = false;
    this.value = undefined;
    this.pageNonce = undefined;
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
      epoch === this.epoch &&
      this.options.nonce() === this.pageNonce
    );
  }
  private lock(): void {
    for (const control of this.options.fields.querySelectorAll<
      | HTMLInputElement
      | HTMLTextAreaElement
      | HTMLButtonElement
      | HTMLSelectElement
    >("input,textarea,button,select"))
      control.disabled = this.busy;
    this.batch?.setBusy(this.busy);
    this.refreshImport?.();
  }
  private async load(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce) {
      uiText(this.options.status, "工作区尚未就绪，请稍后重新打开");
      return;
    }
    this.pageNonce = nonce;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.options.fields.replaceChildren();
    uiText(this.options.status, "正在读取私钥身份…");
    try {
      const [value, enabled] = await Promise.all([
        this.dependencies.read(nonce, operation.signal),
        this.options.imports
          ?.available(nonce, operation.signal)
          .catch(() => false) ?? false,
      ]);
      if (!this.current(epoch)) return;
      this.value = value;
      this.importEnabled = enabled;
      this.render();
      uiText(
        this.options.status,
        value.vaultPresent
          ? "选择私钥身份，或添加名称与用户名"
          : "请先创建保险库，再添加私钥身份",
      );
    } catch {
      if (this.current(epoch))
        uiText(this.options.status, "无法读取私钥身份，关闭后重新打开");
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
    this.clearKeyInputs();
    this.options.fields.replaceChildren();
    const generation = ++this.renderGeneration,
      epoch = this.epoch;
    if (!value.vaultPresent) {
      const empty = document.createElement("p");
      uiText(empty, "保险库尚未创建，暂时不能添加私钥身份。");
      this.options.fields.append(empty);
      return;
    }
    const label = document.createElement("label");
    uiText(label, "编辑私钥身份 ");
    const select = document.createElement("select");
    uiAttribute(select, "aria-label", "编辑私钥身份");
    const create = document.createElement("option");
    create.value = "";
    uiText(create, "添加私钥身份");
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
    this.form(
      value.identities.find((identity) => identity.id === select.value),
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
      kind: "私钥身份",
      consequence: "名称和引用会被删除，已保存的密文保留。现有会话会继续运行。",
      current: () =>
        this.current(epoch) &&
        this.value === value &&
        !this.busy &&
        this.renderGeneration === generation,
      edit: (id) => {
        this.render(id);
        this.lock();
      },
      filter: () => this.clearKeyInputs(),
      remove: (ids) => {
        void this.save({ action: "remove_many", ids });
      },
    });
  }
  private form(
    row: KeyIdentity | undefined,
    value: KeyIdentityCatalogue,
    generation: number,
    epoch: number,
  ): void {
    const id = row?.id,
      initial: IdentityFields = row ?? { label: "", username: "" };
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
    let passphrase: HTMLSelectElement | undefined;
    if (!id) {
      const label = document.createElement("label");
      uiText(label, "私钥需要解密口令");
      passphrase = document.createElement("select");
      uiAttribute(passphrase, "aria-label", "私钥需要解密口令");
      for (const [value, caption] of [
        ["no", "不需要"],
        ["yes", "需要"],
      ] as const) {
        const option = document.createElement("option");
        option.value = value;
        uiText(option, caption);
        passphrase.append(option);
      }
      passphrase.value = "no";
      label.append(passphrase);
      form.append(label);
    } else {
      const state = document.createElement("p");
      uiText(
        state,
        row.passphrase
          ? message(
              "{0} · {1}",
              message(row.secret.hasValue ? "私钥：已保存" : "私钥：未保存"),
              message(
                row.passphrase.hasValue
                  ? "解密口令：已保存"
                  : "解密口令：未保存",
              ),
            )
          : row.secret.hasValue
            ? "私钥：已保存"
            : "私钥：未保存",
      );
      form.append(state);
    }
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
      try {
        if (!id && passphrase?.value !== "yes" && passphrase?.value !== "no")
          throw Error("key-identities.invalid");
        const change: KeyIdentityChange = id
          ? { action: "update", id, ...fields }
          : {
              action: "create",
              ...fields,
              withPassphrase: passphrase?.value === "yes",
            };
        validateKeyIdentityChange(change);
        void this.save(change);
      } catch {
        uiText(
          this.options.status,
          "请检查名称和用户名；用户名不能包含空白或以连字符开头",
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
    if (row && this.importEnabled && this.options.imports)
      this.importForm(row, valid);
  }
  private importForm(row: KeyIdentity, valid: () => boolean): void {
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const hint = document.createElement("p");
    hint.className = "workspace-key-input";
    uiText(
      hint,
      "先解锁保险库，再粘贴 OpenSSH 或 PKCS#8 PEM 格式的 Ed25519 或 ECDSA（P-256、P-384、P-521）私钥。保存后输入会清除。",
    );
    form.append(hint);
    const label = document.createElement("label");
    label.className = "workspace-key-input";
    uiText(label, "私钥");
    const key = document.createElement("textarea");
    key.rows = 5;
    key.maxLength = 65536;
    key.required = true;
    key.autocomplete = "off";
    key.spellcheck = false;
    key.setAttribute("autocapitalize", "off");
    uiAttribute(key, "aria-label", "私钥内容");
    this.keyInputs.push(key);
    label.append(key);
    form.append(label);
    let passphrase: HTMLInputElement | undefined;
    if (row.passphrase) {
      const label = document.createElement("label");
      uiText(label, "私钥解密口令");
      passphrase = document.createElement("input");
      passphrase.type = "password";
      passphrase.required = true;
      passphrase.maxLength = 4096;
      passphrase.autocomplete = "off";
      passphrase.spellcheck = false;
      uiAttribute(passphrase, "aria-label", "私钥解密口令");
      this.keyInputs.push(passphrase);
      label.append(passphrase);
      form.append(label);
    }
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const save = document.createElement("button");
    save.type = "submit";
    uiText(save, row.secret.hasValue ? "替换私钥" : "保存私钥");
    actions.append(save);
    form.append(actions);
    const refresh = () => {
      const keyLength = new TextEncoder().encode(key.value).length;
      const passLength = passphrase
        ? new TextEncoder().encode(passphrase.value).length
        : undefined;
      save.disabled =
        this.busy ||
        keyLength === 0 ||
        keyLength > 65536 ||
        (passLength !== undefined && (passLength === 0 || passLength > 4096));
    };
    this.refreshImport = refresh;
    key.addEventListener("input", refresh);
    passphrase?.addEventListener("input", refresh);
    refresh();
    form.addEventListener("submit", (event) => {
      event.preventDefault();
      if (!valid()) return;
      refresh();
      if (save.disabled) return;
      const bytes = new TextEncoder().encode(key.value);
      const pass = passphrase
        ? new TextEncoder().encode(passphrase.value)
        : undefined;
      key.value = "";
      if (passphrase) passphrase.value = "";
      void this.saveKey(row.id, bytes, pass);
    });
    this.options.fields.append(form);
  }
  private async saveKey(
    identity: string,
    bytes: Uint8Array,
    pass: Uint8Array | undefined,
  ): Promise<void> {
    const nonce = this.pageNonce,
      epoch = this.epoch,
      importer = this.options.imports;
    if (!nonce || !importer || !this.current(epoch) || this.busy) {
      bytes.fill(0);
      pass?.fill(0);
      return;
    }
    const operation = new AbortController();
    this.operation?.abort();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.lock();
    uiText(this.options.status, "正在保存私钥…");
    try {
      await importer.save(nonce, identity, bytes, pass, operation.signal);
      if (!this.current(epoch)) return;
      const next = await this.dependencies.read(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.value = next;
      this.render(identity);
      uiText(this.options.status, "私钥已保存，输入已清除");
      this.options.done.focus();
    } catch {
      if (this.current(epoch)) {
        this.clearKeyInputs();
        this.value = undefined;
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "保存未确认，输入已清除。请解锁保险库或关闭后重新打开，以检查结果",
        );
        this.options.done.focus();
      }
    } finally {
      bytes.fill(0);
      pass?.fill(0);
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
        this.lock();
      }
    }
  }
  private async save(change: KeyIdentityChange): Promise<void> {
    const value = this.value,
      nonce = this.pageNonce,
      epoch = this.epoch;
    if (!value || !nonce || !this.current(epoch) || this.busy) return;
    const operation = new AbortController();
    this.operation?.abort();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.lock();
    uiText(this.options.status, "正在保存私钥身份…");
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
      uiText(this.options.status, "私钥身份已保存");
    } catch {
      if (this.current(epoch)) {
        this.value = undefined;
        this.options.fields.replaceChildren();
        uiText(
          this.options.status,
          "保存未确认或内容已变化，关闭后重新打开以检查结果",
        );
        this.options.done.focus();
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
