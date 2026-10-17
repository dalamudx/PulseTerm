import { validateHostChange } from "./workspace-hosts";
import { uiText } from "./workspace-language";
import {
  cancelTemporaryTarget,
  saveTemporaryTarget,
  type TemporaryTarget,
} from "./workspace-temporary-target";
export class WorkspaceQuickSaveDialog {
  private epoch = 0;
  private nonce: string | undefined;
  private target: TemporaryTarget | undefined;
  private operation: AbortController | undefined;
  private busy = false;
  private focus: HTMLElement | undefined;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      label: HTMLInputElement;
      description: HTMLElement;
      status: HTMLElement;
      save: HTMLButtonElement;
      cancel: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(value: boolean): void;
      saved(nonce: string, id: string): Promise<void>;
    },
    private readonly dependencies = {
      save: saveTemporaryTarget,
      cancel: cancelTemporaryTarget,
    },
  ) {
    options.save.addEventListener(
      "click",
      () => {
        void this.save();
      },
      { signal: options.signal },
    );
    options.cancel.addEventListener("click", () => this.close(), {
      signal: options.signal,
    });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal: options.signal },
    );
    options.signal.addEventListener("abort", () => this.close(), {
      once: true,
    });
  }
  open(nonce: string, target: TemporaryTarget): void {
    if (
      this.options.signal.aborted ||
      this.options.nonce() !== nonce ||
      this.options.dialog.open
    ) {
      void this.dependencies.cancel(nonce, target.token);
      return;
    }
    ++this.epoch;
    this.nonce = nonce;
    this.target = target;
    this.operation = new AbortController();
    this.busy = false;
    const focus = this.options.dialog.ownerDocument.activeElement;
    this.focus = focus instanceof HTMLElement ? focus : undefined;
    this.options.label.value = target.target.label;
    this.options.label.disabled = false;
    this.options.save.disabled = false;
    const host = target.target.host.includes(":")
      ? `[${target.target.host}]`
      : target.target.host;
    this.options.description.textContent = `${target.target.username}@${host}:${target.target.port}`;
    uiText(this.options.status, "连接已建立。保存后可从主机列表重新连接。");
    this.options.dialog.showModal();
    this.options.label.focus();
  }
  private current(epoch: number): boolean {
    return (
      this.epoch === epoch &&
      this.options.dialog.open &&
      !this.options.signal.aborted &&
      !this.operation?.signal.aborted &&
      this.nonce === this.options.nonce()
    );
  }
  private async save(): Promise<void> {
    const target = this.target,
      nonce = this.nonce,
      operation = this.operation,
      epoch = this.epoch;
    if (!target || !nonce || !operation || this.busy || !this.current(epoch))
      return;
    try {
      validateHostChange({
        action: "create",
        label: this.options.label.value,
        host: target.target.host,
        port: target.target.port,
        username: target.target.username,
      });
    } catch {
      uiText(this.options.status, "请输入有效的主机名称。");
      this.options.label.focus();
      return;
    }
    this.busy = true;
    this.options.label.disabled = true;
    this.options.save.disabled = true;
    this.options.working(true);
    uiText(this.options.status, "正在保存主机…");
    try {
      const id = await this.dependencies.save(
        nonce,
        target,
        this.options.label.value,
        operation.signal,
      );
      if (!this.current(epoch)) return;
      this.close();
      await this.options.saved(nonce, id);
    } catch {
      if (this.current(epoch)) {
        uiText(
          this.options.status,
          "保存结果未确认，请关闭后检查主机列表。不会重复保存，已有连接继续运行。",
        );
        this.options.cancel.focus();
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
      }
    }
  }
  close(): void {
    const nonce = this.nonce,
      target = this.target;
    const same = nonce !== undefined && nonce === this.options.nonce();
    ++this.epoch;
    this.operation?.abort();
    this.operation = undefined;
    this.target = undefined;
    this.nonce = undefined;
    this.busy = false;
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.save.disabled = true;
    this.options.label.value = "";
    this.options.description.textContent = "";
    uiText(this.options.status, "");
    if (nonce && target) void this.dependencies.cancel(nonce, target.token);
    if (same && !this.options.signal.aborted) {
      this.options.working(false);
      if (this.focus?.isConnected) this.focus.focus();
    }
    this.focus = undefined;
  }
}
