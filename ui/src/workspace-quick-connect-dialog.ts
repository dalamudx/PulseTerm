import {
  type HostCatalogue,
  type HostFields,
  readHosts,
  saveHost,
} from "./workspace-hosts";
import { message, uiText } from "./workspace-language";
import {
  matchingQuickHosts,
  parseQuickTarget,
} from "./workspace-quick-connect";
import {
  cancelTemporaryTarget,
  prepareTemporaryTarget,
  type TemporaryTarget,
} from "./workspace-temporary-target";
/** Saving or minting metadata never grants dial, trust or credential authority. */
export class WorkspaceQuickConnectDialog {
  private epoch = 0;
  private nonce: string | undefined;
  private operation: AbortController | undefined;
  private catalogue: HostCatalogue | undefined;
  private preview: HostFields | undefined;
  private busy = false;
  private focus: HTMLElement | undefined;
  constructor(
    private readonly options: {
      dialog: HTMLDialogElement;
      address: HTMLInputElement;
      label: HTMLInputElement;
      inspect: HTMLButtonElement;
      continue: HTMLButtonElement;
      temporary?: HTMLButtonElement;
      cancel: HTMLButtonElement;
      preview: HTMLElement;
      status: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      enabled(): boolean;
      working(value: boolean): void;
      changed(value: HostCatalogue): void;
      select(id: string): void;
      selectTemporary?(nonce: string, target: TemporaryTarget): void;
    },
    private readonly dependencies: {
      read: typeof readHosts;
      save: typeof saveHost;
      temporary?: typeof prepareTemporaryTarget;
    } = { read: readHosts, save: saveHost, temporary: prepareTemporaryTarget },
  ) {
    const signal = options.signal;
    options.inspect.addEventListener("click", () => this.inspect(), { signal });
    options.continue.addEventListener(
      "click",
      () => {
        void this.continue();
      },
      { signal },
    );
    options.temporary?.addEventListener(
      "click",
      () => {
        void this.connectTemporary();
      },
      { signal },
    );
    options.cancel.addEventListener("click", () => this.close(), { signal });
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      { signal },
    );
    for (const input of [options.address, options.label])
      input.addEventListener(
        "input",
        () => {
          this.preview = undefined;
          options.preview.textContent = "";
          options.continue.disabled = true;
          if (options.temporary) options.temporary.disabled = true;
        },
        { signal },
      );
    signal.addEventListener("abort", () => this.close(), { once: true });
  }
  private current(epoch: number): boolean {
    return (
      !this.options.signal.aborted &&
      this.options.dialog.open &&
      this.epoch === epoch &&
      this.nonce === this.options.nonce() &&
      !this.operation?.signal.aborted
    );
  }
  async open(): Promise<void> {
    if (
      !this.options.enabled() ||
      this.options.signal.aborted ||
      this.options.dialog.open
    )
      return;
    const nonce = this.options.nonce();
    if (!nonce) return;
    this.nonce = nonce;
    const epoch = ++this.epoch;
    this.operation = new AbortController();
    const focused = this.options.dialog.ownerDocument.activeElement;
    this.focus = focused instanceof HTMLElement ? focused : undefined;
    this.catalogue = undefined;
    this.preview = undefined;
    this.options.address.value = "";
    this.options.label.value = "";
    this.options.preview.textContent = "";
    this.options.continue.disabled = true;
    if (this.options.temporary) this.options.temporary.disabled = true;
    this.options.inspect.disabled = true;
    this.options.dialog.showModal();
    this.busy = true;
    this.options.address.disabled = true;
    this.options.label.disabled = true;
    this.options.working(true);
    uiText(this.options.status, "正在读取保存的主机…");
    try {
      const value = await this.dependencies.read(nonce, this.operation.signal);
      if (!this.current(epoch)) return;
      this.catalogue = value;
      uiText(this.options.status, "输入地址后核对目标，再保存或使用已有主机。");
      this.options.address.disabled = false;
      this.options.label.disabled = false;
      this.options.inspect.disabled = false;
      this.options.address.focus();
    } catch {
      if (this.current(epoch))
        uiText(this.options.status, "目录读取失败，请关闭后重新打开。");
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.address.disabled = false;
        this.options.label.disabled = false;
        this.options.working(false);
      }
    }
  }
  private inspect(): void {
    if (this.busy || !this.current(this.epoch) || !this.catalogue) return;
    this.preview = undefined;
    this.options.continue.disabled = true;
    if (this.options.temporary) this.options.temporary.disabled = true;
    try {
      const target = parseQuickTarget(
        this.options.address.value,
        this.options.label.value,
      );
      const matches = matchingQuickHosts(this.catalogue.hosts, target);
      if (matches.length > 1) {
        uiText(
          this.options.status,
          "多个已保存主机使用此地址，请从主机列表选择。",
        );
        return;
      }
      const display = target.host.includes(":")
        ? `[${target.host}]`
        : target.host;
      uiText(
        this.options.preview,
        message(
          "用户名：{0}\n主机：{1}\n端口：{2}\n名称：{3}",
          target.username,
          display,
          target.port,
          matches[0]?.label ?? target.label,
        ),
      );
      uiText(
        this.options.status,
        matches.length
          ? "将使用已保存主机，随后选择身份并核对指纹。"
          : "将保存新主机，随后选择身份并核对指纹。",
      );
      uiText(
        this.options.continue,
        matches.length ? "使用此主机" : "保存并继续",
      );
      this.options.continue.disabled = false;
      if (this.options.temporary)
        this.options.temporary.disabled = !this.options.selectTemporary;
      this.preview = target;
    } catch {
      this.options.preview.textContent = "";
      uiText(
        this.options.status,
        "请输入 user@host:port；IPv6 使用方括号。不支持密码、路径或其他协议。",
      );
    }
  }
  private async connectTemporary(): Promise<void> {
    const target = this.preview,
      nonce = this.nonce,
      operation = this.operation,
      epoch = this.epoch;
    if (
      this.busy ||
      !target ||
      !nonce ||
      !operation ||
      !this.options.selectTemporary ||
      !this.current(epoch)
    )
      return;
    try {
      if (
        JSON.stringify(
          parseQuickTarget(
            this.options.address.value,
            this.options.label.value,
          ),
        ) !== JSON.stringify(target)
      ) {
        this.inspect();
        return;
      }
    } catch {
      this.inspect();
      return;
    }
    this.busy = true;
    this.options.address.disabled = true;
    this.options.label.disabled = true;
    this.options.continue.disabled = true;
    this.options.inspect.disabled = true;
    if (this.options.temporary) this.options.temporary.disabled = true;
    this.options.working(true);
    uiText(this.options.status, "正在核对临时目标…");
    try {
      const receipt = await (
        this.dependencies.temporary ?? prepareTemporaryTarget
      )(nonce, target, operation.signal);
      if (!this.current(epoch)) {
        void cancelTemporaryTarget(nonce, receipt.token);
        return;
      }
      this.close();
      if (!this.options.signal.aborted && this.options.nonce() === nonce)
        this.options.selectTemporary(nonce, receipt);
      else void cancelTemporaryTarget(nonce, receipt.token);
    } catch {
      if (this.current(epoch)) {
        this.preview = undefined;
        uiText(
          this.options.status,
          "临时目标未确认，请关闭后重新核对地址。未开始连接。",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.address.disabled = false;
        this.options.label.disabled = false;
        this.options.working(false);
      }
    }
  }

  private async continue(): Promise<void> {
    const target = this.preview,
      catalogue = this.catalogue,
      nonce = this.nonce,
      operation = this.operation,
      epoch = this.epoch;
    if (
      this.busy ||
      !target ||
      !catalogue ||
      !nonce ||
      !operation ||
      !this.current(epoch)
    )
      return;
    try {
      const current = parseQuickTarget(
        this.options.address.value,
        this.options.label.value,
      );
      if (JSON.stringify(current) !== JSON.stringify(target)) {
        this.inspect();
        return;
      }
    } catch {
      this.inspect();
      return;
    }
    this.busy = true;
    this.options.address.disabled = true;
    this.options.label.disabled = true;
    this.options.working(true);
    this.options.continue.disabled = true;
    if (this.options.temporary) this.options.temporary.disabled = true;
    this.options.inspect.disabled = true;
    uiText(this.options.status, "正在确认保存的主机…");
    try {
      const matches = matchingQuickHosts(catalogue.hosts, target);
      let id = matches.length === 1 ? matches[0]?.id : undefined;
      if (!id) {
        const next = await this.dependencies.save(
          nonce,
          catalogue.token,
          { action: "create", ...target },
          operation.signal,
        );
        if (!this.current(epoch)) return;
        const created = next.hosts.filter(
          (host) => !catalogue.hosts.some((old) => old.id === host.id),
        );
        if (created.length !== 1) throw Error("quick_connect.unconfirmed");
        id = created[0]?.id;
        this.options.changed(next);
      }
      if (!id || !this.current(epoch)) return;
      this.close();
      if (!this.options.signal.aborted && this.options.nonce() === nonce)
        this.options.select(id);
    } catch {
      if (this.current(epoch)) {
        this.preview = undefined;
        this.catalogue = undefined;
        uiText(
          this.options.status,
          "保存未确认或目录已变化，请关闭后重新读取。已有保存可能已完成，不会重试或连接。",
        );
      }
    } finally {
      if (this.current(epoch)) {
        this.busy = false;
        this.options.address.disabled = false;
        this.options.label.disabled = false;
        this.options.working(false);
      }
    }
  }
  close(): void {
    const same = this.nonce === this.options.nonce();
    ++this.epoch;
    this.operation?.abort();
    this.operation = undefined;
    this.catalogue = undefined;
    this.preview = undefined;
    this.busy = false;
    this.nonce = undefined;
    this.options.address.value = "";
    this.options.label.value = "";
    this.options.preview.textContent = "";
    this.options.continue.disabled = true;
    if (this.options.temporary) this.options.temporary.disabled = true;
    this.options.inspect.disabled = true;
    if (this.options.dialog.open) this.options.dialog.close();
    this.options.address.disabled = false;
    this.options.label.disabled = false;
    if (same && !this.options.signal.aborted) this.options.working(false);
    if (same && !this.options.signal.aborted && this.focus?.isConnected)
      this.focus.focus();
    this.focus = undefined;
  }
}
