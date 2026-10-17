import { message, uiText } from "./workspace-language";
import {
  type ConfigPreview,
  checkConfigSource,
  confirmConfigPreview,
  readConfigPreview,
  validateConfigAlias,
} from "./workspace-ssh-config";
import {
  type ConfigImportPreview,
  cancelConfigImport,
  prepareConfigImport,
  saveConfigImport,
  validateImportLabel,
} from "./workspace-ssh-config-import";
import { ConfigSourceMonitor } from "./workspace-ssh-config-monitor";
export class WorkspaceConfigDialog {
  private epoch = 0;
  private busy = false;
  private disposed = false;
  private value: ConfigPreview | undefined;
  private valueNonce: string | undefined;
  private approval: ConfigImportPreview | undefined;
  private approvalNonce: string | undefined;
  private operation: AbortController | undefined;
  private readonly monitor: ConfigSourceMonitor | undefined;
  private sourceChecking = false;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      form: HTMLFormElement;
      alias: HTMLInputElement;
      read: HTMLButtonElement;
      confirm: HTMLButtonElement;
      fields: HTMLElement;
      status: HTMLElement;
      importControls?: {
        label: HTMLInputElement;
        prepare: HTMLButtonElement;
        save: HTMLButtonElement;
      };
      signal: AbortSignal;
      nonce(): string | undefined;
      enabled(): boolean;
      working(value: boolean): void;
      restoreFocus?(): void;
    },
    private readonly dependencies: {
      read: typeof readConfigPreview;
      confirm: typeof confirmConfigPreview;
      prepare?: typeof prepareConfigImport;
      save?: typeof saveConfigImport;
      cancel?: typeof cancelConfigImport;
      status?: typeof checkConfigSource;
    } = {
      read: readConfigPreview,
      confirm: confirmConfigPreview,
      status: checkConfigSource,
    },
  ) {
    this.monitor = dependencies.status
      ? new ConfigSourceMonitor({
          check: dependencies.status,
          visible: () => document.visibilityState !== "hidden",
          checking: (value) => {
            this.sourceChecking = value;
            this.update();
          },
          invalid: (reason) => {
            this.value = undefined;
            this.valueNonce = undefined;
            uiText(
              this.options.status,
              reason === "changed"
                ? "配置源已变化，请重新预览。旧值保留供核对，不能保存。"
                : "源状态未确认，请重新预览后保存。旧值保留供核对。",
            );
            this.update();
          },
        })
      : undefined;
    options.signal.addEventListener("abort", this.abort, { once: true });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    options.form.addEventListener(
      "submit",
      (event) => {
        event.preventDefault();
        void this.read();
      },
      { signal: options.signal },
    );
    options.alias.addEventListener(
      "input",
      () => {
        this.invalidate();
        uiText(this.options.status, "");
        this.update();
      },
      { signal: options.signal },
    );
    options.confirm.addEventListener(
      "click",
      () => {
        void this.confirm();
      },
      { signal: options.signal },
    );
    const controls = options.importControls;
    if (controls) {
      controls.label.addEventListener(
        "input",
        () => {
          this.invalidate();
          uiText(this.options.status, "名称已修改，请重新预览。");
          this.update();
        },
        { signal: options.signal },
      );
      controls.prepare.addEventListener(
        "click",
        () => {
          void this.prepareImport();
        },
        { signal: options.signal },
      );
      controls.save.addEventListener(
        "click",
        () => {
          void this.saveImport();
        },
        { signal: options.signal },
      );
    }
    this.update();
  }
  open(): void {
    if (
      this.disposed ||
      this.options.signal.aborted ||
      !this.options.enabled() ||
      !this.options.nonce() ||
      this.options.dialog.open
    )
      return;
    this.invalidate();
    this.options.alias.value = "";
    if (this.options.importControls)
      this.options.importControls.label.value = "";
    uiText(this.options.status, "输入 Host 别名以读取配置。");
    this.options.dialog.showModal();
    this.update();
    this.options.alias.focus();
  }
  close(): void {
    this.invalidate();
    this.options.alias.value = "";
    uiText(this.options.status, "");
    if (this.options.dialog.open) this.options.dialog.close();
    this.update();
    if (!this.disposed && !this.options.signal.aborted)
      this.options.restoreFocus?.();
  }
  private invalidate(): void {
    this.epoch++;
    this.monitor?.stop();
    this.operation?.abort();
    this.operation = undefined;
    if (this.approval && this.approvalNonce)
      void (this.dependencies.cancel ?? cancelConfigImport)(
        this.approvalNonce,
        this.approval.token,
      );
    this.approval = undefined;
    this.approvalNonce = undefined;
    this.value = undefined;
    this.valueNonce = undefined;
    this.busy = false;
    this.options.fields.replaceChildren();
    this.options.working(false);
  }
  private startSourceMonitor(): void {
    const value = this.value,
      nonce = this.valueNonce,
      epoch = this.epoch;
    if (value && nonce)
      this.monitor?.start(
        nonce,
        value.token,
        () => this.current(epoch, nonce) && this.value === value && !this.busy,
      );
  }
  private current(epoch: number, nonce: string): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      this.options.enabled() &&
      this.options.nonce() === nonce &&
      this.epoch === epoch
    );
  }
  private update(): void {
    let valid = false;
    try {
      validateConfigAlias(this.options.alias.value);
      valid = true;
    } catch {}
    this.options.read.disabled = this.busy || !valid || this.disposed;
    this.options.confirm.disabled =
      this.busy || this.sourceChecking || !this.value || this.disposed;
    const controls = this.options.importControls;
    if (controls) {
      let label = false;
      try {
        validateImportLabel(controls.label.value);
        label = true;
      } catch {}
      controls.prepare.disabled =
        this.busy ||
        this.sourceChecking ||
        !this.value ||
        !label ||
        this.disposed;
      controls.save.disabled = this.busy || !this.approval || this.disposed;
    }
  }
  private render(value: ConfigPreview): void {
    const list = document.createElement("dl");
    list.className = "workspace-fingerprint-fields";
    for (const [label, text] of [
      ["主机地址", value.host],
      ["端口", String(value.port)],
      ["用户名", value.username],
      [
        "IdentityFile 提示",
        message("{0} 项（不读取密钥）", value.identityFileCount),
      ],
    ] as const) {
      const term = document.createElement("dt"),
        definition = document.createElement("dd");
      uiText(term, label);
      if (typeof text === "object") uiText(definition, text);
      else definition.textContent = text;
      list.append(term, definition);
    }
    this.options.fields.replaceChildren(list);
  }
  private async read(): Promise<void> {
    const nonce = this.options.nonce();
    if (!nonce || this.busy || !this.current(this.epoch, nonce)) return;
    try {
      validateConfigAlias(this.options.alias.value);
    } catch {
      uiText(this.options.status, "请输入 Host 别名，不包含空格或通配符。");
      return;
    }
    const alias = this.options.alias.value;
    this.invalidate();
    const epoch = this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.update();
    uiText(this.options.status, "正在读取配置…");
    try {
      const value = await this.dependencies.read(
        nonce,
        alias,
        operation.signal,
      );
      if (!this.current(epoch, nonce)) return;
      this.value = value;
      this.valueNonce = nonce;
      if (
        this.options.importControls &&
        !this.options.importControls.label.value
      )
        this.options.importControls.label.value = alias;
      this.render(value);
      uiText(
        this.options.status,
        "已读取。可以核对源文件是否仍与本次预览一致。",
      );
    } catch {
      if (this.current(epoch, nonce))
        uiText(
          this.options.status,
          "配置不可用或读取未确认，请检查文件后重新预览。不会使用空配置。",
        );
    } finally {
      if (this.current(epoch, nonce)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.update();
        this.startSourceMonitor();
      }
    }
  }
  private async confirm(): Promise<void> {
    const value = this.value,
      nonce = this.valueNonce;
    if (!value || !nonce || this.busy || !this.current(this.epoch, nonce))
      return;
    if (this.sourceChecking) return;
    this.monitor?.stop();
    this.value = undefined;
    this.valueNonce = undefined;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.update();
    uiText(this.options.status, "正在核对源文件…");
    try {
      await this.dependencies.confirm(nonce, value.token, operation.signal);
      if (this.current(epoch, nonce))
        uiText(
          this.options.status,
          "本次预览的源文件未变。没有保存主机或建立连接。",
        );
    } catch {
      if (this.current(epoch, nonce))
        uiText(
          this.options.status,
          "配置已变化或核对未确认，请重新预览。没有保存主机或建立连接。",
        );
    } finally {
      if (this.current(epoch, nonce)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.update();
      }
    }
  }
  private async prepareImport(): Promise<void> {
    const value = this.value,
      nonce = this.valueNonce,
      controls = this.options.importControls;
    if (
      !value ||
      !nonce ||
      !controls ||
      this.busy ||
      !this.current(this.epoch, nonce)
    )
      return;
    if (this.sourceChecking) return;
    this.monitor?.stop();
    let label: string;
    try {
      label = validateImportLabel(controls.label.value);
    } catch {
      return;
    }
    this.value = undefined;
    this.valueNonce = undefined;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.update();
    uiText(this.options.status, "正在准备保存…");
    try {
      const prepared = await (this.dependencies.prepare ?? prepareConfigImport)(
        nonce,
        value.token,
        label,
        operation.signal,
      );
      if (!this.current(epoch, nonce)) {
        void (this.dependencies.cancel ?? cancelConfigImport)(
          nonce,
          prepared.token,
        );
        return;
      }
      this.approval = prepared;
      this.approvalNonce = nonce;
      this.render(prepared);
      uiText(
        this.options.status,
        message(
          "已准备主机“{0}”。保存后可在 SSH 主机中查看，不会建立连接。",
          prepared.label,
        ),
      );
    } catch {
      if (this.current(epoch, nonce))
        uiText(this.options.status, "准备未确认，请重新预览。没有保存主机。");
    } finally {
      if (this.current(epoch, nonce)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.update();
        if (this.approval) controls.save.focus();
      }
    }
  }
  private async saveImport(): Promise<void> {
    const value = this.approval,
      nonce = this.approvalNonce;
    if (!value || !nonce || this.busy || !this.current(this.epoch, nonce))
      return;
    this.approval = undefined;
    this.approvalNonce = undefined;
    const epoch = ++this.epoch,
      operation = new AbortController();
    this.operation = operation;
    this.busy = true;
    this.options.working(true);
    this.update();
    uiText(this.options.status, "正在保存主机…");
    try {
      await (this.dependencies.save ?? saveConfigImport)(
        nonce,
        value.token,
        operation.signal,
      );
      if (this.current(epoch, nonce))
        uiText(
          this.options.status,
          message("已保存主机“{0}”。可在 SSH 主机中查看。", value.label),
        );
    } catch {
      if (this.current(epoch, nonce))
        uiText(
          this.options.status,
          "保存未确认或来源已变化，请先查看主机目录，再重新预览。不会自动重试。",
        );
    } finally {
      if (this.current(epoch, nonce)) {
        this.busy = false;
        this.operation = undefined;
        this.options.working(false);
        this.update();
      }
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.options.signal.removeEventListener("abort", this.abort);
    this.close();
  }
}
