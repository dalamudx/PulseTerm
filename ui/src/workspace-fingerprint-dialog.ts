import { uiText } from "./workspace-language";
import {
  type Fingerprint,
  FingerprintTicket,
} from "./workspace-ssh-fingerprint";

type Client = Pick<FingerprintTicket, "observe" | "confirm" | "cancel">;
export class WorkspaceFingerprintDialog {
  private epoch = 0;
  private disposed = false;
  private operation: AbortController | undefined;
  private client: Client | undefined;
  private ready = false;
  private busy = false;
  private focus: HTMLElement | undefined;
  private originalNonce: string | undefined;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      fields: HTMLElement;
      status: HTMLElement;
      cancel: HTMLButtonElement;
      approve: HTMLButtonElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      working(busy: boolean): void;
    },
    private readonly dependencies: {
      prepare(
        nonce: string,
        host: string,
        signal: AbortSignal,
      ): Promise<Client>;
    } = { prepare: FingerprintTicket.prepare },
  ) {
    options.dialog.addEventListener("cancel", (event) => {
      event.preventDefault();
      this.close();
    });
    options.cancel.addEventListener("click", () => this.close());
    options.approve.addEventListener("click", () => {
      void this.confirm();
    });
    options.signal.addEventListener("abort", () => this.dispose(), {
      once: true,
    });
  }
  async open(host: string): Promise<void> {
    if (
      this.disposed ||
      this.options.signal.aborted ||
      this.options.dialog.open
    )
      return;
    const nonce = this.options.nonce();
    if (!nonce) return;
    this.originalNonce = nonce;
    const epoch = ++this.epoch;
    const operation = new AbortController();
    this.operation = operation;
    const focused = this.options.dialog.ownerDocument.activeElement;
    this.focus = focused instanceof HTMLElement ? focused : undefined;
    this.client = undefined;
    this.ready = false;
    this.busy = true;
    this.options.approve.disabled = true;
    this.options.fields.replaceChildren();
    uiText(this.options.status, "正在核对保存的主机指纹 · 不发送认证");
    uiText(this.options.cancel, "取消");
    this.options.dialog.showModal();
    this.options.cancel.focus();
    this.options.working(true);
    try {
      const client = await this.dependencies.prepare(
        nonce,
        host,
        operation.signal,
      );
      if (!this.current(epoch)) {
        client.cancel();
        return;
      }
      this.client = client;
      const value = await client.observe();
      if (!this.current(epoch)) return;
      this.render(value);
      this.ready = value.status === "unknown";
      this.options.approve.disabled = !this.ready;
    } catch {
      if (this.current(epoch)) {
        this.ready = false;
        this.options.approve.disabled = true;
        uiText(
          this.options.status,
          "指纹核对未确认 · 不自动重试，关闭后重新核对",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
      }
    }
  }
  private current(epoch: number): boolean {
    return (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      epoch === this.epoch &&
      this.originalNonce === this.options.nonce()
    );
  }
  private render(value: Fingerprint): void {
    const document = this.options.dialog.ownerDocument;
    const details = document.createElement("dl");
    details.className = "workspace-fingerprint-fields";
    for (const [name, text] of [
      ["保存的主机", value.label],
      ["目标", `${value.host}:${value.port}`],
      ["密钥算法", value.algorithm],
      ["SHA-256 指纹", value.fingerprint],
    ] as const) {
      const title = document.createElement("dt"),
        content = document.createElement("dd");
      uiText(title, name);
      content.textContent = text;
      if (name === "SHA-256 指纹") {
        content.className = "workspace-fingerprint-value";
        content.tabIndex = 0;
      }
      details.append(title, content);
    }
    this.options.fields.replaceChildren(details);
    uiText(
      this.options.status,
      value.status === "unknown"
        ? "首次见到此密钥。请通过独立可信渠道核对，保存仅添加应用指纹记录，不启动连接。"
        : value.status === "changed"
          ? "主机密钥已变更 · 本入口禁止替换。请通过可信渠道核对现有记录。"
          : "已匹配应用保存的指纹 · 本次仅核对，没有认证或连接。",
    );
    if (value.status !== "unknown") uiText(this.options.cancel, "关闭");
  }
  private async confirm(): Promise<void> {
    const client = this.client,
      epoch = this.epoch;
    if (!client || !this.ready || this.busy || !this.current(epoch)) return;
    this.ready = false;
    this.busy = true;
    this.options.approve.disabled = true;
    this.options.working(true);
    uiText(this.options.status, "正在保存此指纹 · 不自动连接");
    try {
      await client.confirm();
      if (!this.current(epoch)) return;
      uiText(this.options.status, "已保存此指纹 · 没有启动 SSH 连接");
      uiText(this.options.cancel, "完成");
      this.options.cancel.focus();
    } catch {
      if (this.current(epoch)) {
        uiText(
          this.options.status,
          "保存未确认或来源已变化 · 不自动重试，关闭后重新核对实际记录",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.working(false);
      }
    }
  }
  close(): void {
    ++this.epoch;
    this.ready = false;
    this.busy = false;
    this.operation?.abort();
    this.operation = undefined;
    this.client?.cancel();
    this.client = undefined;
    this.options.approve.disabled = true;
    this.options.fields.replaceChildren();
    this.options.status.textContent = "";
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.working(false);
    if (
      !this.disposed &&
      !this.options.signal.aborted &&
      this.originalNonce === this.options.nonce() &&
      this.focus?.isConnected
    )
      this.focus.focus();
    this.focus = undefined;
    this.originalNonce = undefined;
  }
  private dispose(): void {
    this.disposed = true;
    this.close();
  }
}
