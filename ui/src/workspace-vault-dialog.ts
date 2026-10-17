import { uiText } from "./workspace-language";
import {
  lockWorkspaceVault,
  readVaultStatus,
  type VaultStatus,
} from "./workspace-vault";
import { submitMasterPassword } from "./workspace-vault-password";
export class WorkspaceVaultDialog {
  private epoch = 0;
  private busy = false;
  private disposed = false;
  private operation: AbortController | undefined;
  private readonly closed = new Set<() => void>();
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      status: HTMLElement;
      done: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(value: boolean): void;
    },
    private readonly dependencies = {
      read: readVaultStatus,
      lock: lockWorkspaceVault,
      password: submitMasterPassword,
    },
  ) {
    options.signal.addEventListener("abort", () => this.dispose(), {
      once: true,
    });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
  }
  async open() {
    if (this.disposed || this.busy || this.options.dialog.open) return;
    this.options.dialog.showModal();
    await this.load();
  }
  async openAndWait(signal: AbortSignal): Promise<void> {
    if (
      signal.aborted ||
      this.disposed ||
      this.busy ||
      this.options.dialog.open
    ) {
      throw Error("vault.cancelled");
    }
    const waiting = new Promise<void>((resolve) => {
      const done = () => {
        this.closed.delete(done);
        signal.removeEventListener("abort", abort);
        resolve();
      };
      const abort = () => this.close();
      this.closed.add(done);
      signal.addEventListener("abort", abort, { once: true });
    });
    await this.open();
    await waiting;
  }
  private wipe() {
    for (const input of this.options.fields.querySelectorAll<HTMLInputElement>(
      "input",
    ))
      input.value = "";
  }
  close() {
    this.epoch++;
    this.wipe();
    this.operation?.abort();
    this.operation = undefined;
    this.busy = false;
    this.options.fields.replaceChildren();
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.working(false);
    for (const done of this.closed) done();
  }
  dispose() {
    if (this.disposed) return;
    this.close();
    this.disposed = true;
  }
  private current(epoch: number) {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      this.epoch === epoch
    );
  }
  private async request(
    work: (nonce: string, signal: AbortSignal) => Promise<VaultStatus>,
    errorText: string,
  ) {
    if (
      this.disposed ||
      this.busy ||
      this.options.signal.aborted ||
      !this.options.dialog.open
    )
      return;
    const nonce = this.options.nonce();
    if (!nonce) {
      uiText(this.options.status, "页面尚未确认 · 请重新打开");
      return;
    }
    const epoch = ++this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    for (const control of this.options.fields.querySelectorAll<
      HTMLInputElement | HTMLButtonElement
    >("input,button"))
      control.disabled = true;
    uiText(this.options.status, "正在处理…");
    this.options.status.dataset.state = "pending";
    try {
      const value = await work(nonce, operation.signal);
      if (!this.current(epoch)) return;
      this.render(value);
      this.options.done.focus();
    } catch {
      if (this.current(epoch)) {
        uiText(this.options.status, errorText);
        this.options.status.dataset.state = "error";
        this.wipe();
        this.options.fields.replaceChildren(
          this.button("重新读取状态", "workspace-vault-read", () => {
            void this.load();
          }),
        );
        this.options.done.focus();
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        for (const control of this.options.fields.querySelectorAll<
          HTMLInputElement | HTMLButtonElement
        >("input,button"))
          control.disabled = false;
      }
    }
  }
  private load() {
    return this.request(
      this.dependencies.read,
      "无法读取保险库状态 · 请关闭后重新打开，不会创建替代密钥",
    );
  }
  private button(text: string, id: string, action: () => void) {
    const button = document.createElement("button");
    button.type = "button";
    uiText(button, text);
    button.id = id;
    const epoch = this.epoch;
    button.addEventListener(
      "click",
      () => {
        if (this.current(epoch) && !this.busy) action();
      },
      { signal: this.options.signal },
    );
    return button;
  }
  private render(value: VaultStatus) {
    this.wipe();
    this.options.fields.replaceChildren();
    this.options.status.dataset.state = value.state;
    const text = {
      missing: "尚未创建保险库",
      locked: "已锁定",
      available: "已解锁 · 不代表允许 SSH 使用凭据",
      pending: "此前操作尚未确认 · 请重新读取状态",
    };
    uiText(this.options.status, text[value.state]);
    const reload = this.button("重新读取状态", "workspace-vault-read", () => {
      void this.load();
    });
    if (value.state === "available") {
      const lock = this.button("锁定保险库", "workspace-vault-lock", () => {
        void this.request(
          this.dependencies.lock,
          "锁定结果未确认 · 请重新读取状态",
        );
      });
      this.options.fields.append(lock, reload);
      return;
    }
    if (value.state === "pending") {
      this.options.fields.append(reload);
      return;
    }
    const creating = value.state === "missing";
    const form = document.createElement("form");
    form.className = "workspace-host-form";
    const makeInput = (labelText: string, id: string) => {
      const label = document.createElement("label");
      uiText(label, labelText);
      const input = document.createElement("input");
      input.id = id;
      input.type = "password";
      input.autocomplete = creating ? "new-password" : "current-password";
      input.maxLength = 4096;
      input.spellcheck = false;
      input.setAttribute("autocapitalize", "none");
      input.required = true;
      label.append(input);
      form.append(label);
      return input;
    };
    const password = makeInput(
      creating ? "新主密码（至少 12 个字符）" : "主密码",
      "workspace-vault-password",
    );
    const repeat = creating
      ? makeInput("再次输入主密码", "workspace-vault-repeat")
      : undefined;
    const submit = document.createElement("button");
    submit.type = "submit";
    submit.id = "workspace-vault-submit";
    uiText(submit, creating ? "创建并保持锁定" : "解锁保险库");
    form.append(submit);
    const renderEpoch = this.epoch;
    form.addEventListener(
      "submit",
      (event) => {
        event.preventDefault();
        if (this.busy || !this.current(renderEpoch)) return;
        const text = password.value;
        if (
          !text ||
          (creating && (Array.from(text).length < 12 || repeat?.value !== text))
        ) {
          this.wipe();
          uiText(
            this.options.status,
            "主密码至少 12 个字符，且两次输入需完全一致",
          );
          password.focus();
          return;
        }
        const bytes = new TextEncoder().encode(text);
        this.wipe();
        if (bytes.length > 4096) {
          bytes.fill(0);
          uiText(this.options.status, "主密码超过 4096 字节 · 请缩短后重试");
          password.focus();
          return;
        }
        void this.request(
          (nonce, signal) => {
            const wipe = () => bytes.fill(0);
            signal.addEventListener("abort", wipe, { once: true });
            return this.dependencies
              .password(creating ? "create" : "unlock", nonce, bytes, signal)
              .finally(() => signal.removeEventListener("abort", wipe));
          },
          creating
            ? "创建结果未确认 · 请重新读取状态，不会覆盖已有保险库"
            : "未能确认解锁 · 请检查密码或重新读取状态",
        ).finally(() => bytes.fill(0));
      },
      { signal: this.options.signal },
    );
    this.options.fields.append(form, reload);
  }
}
