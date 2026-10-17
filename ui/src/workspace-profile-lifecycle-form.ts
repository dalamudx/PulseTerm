import {
  type UiMessage,
  message as uiMessage,
  uiText,
} from "./workspace-language";
import {
  decodeDeletedProfiles,
  type LifecycleAction,
  type LifecycleOffer,
  ProfileLifecycleEditor,
  type ProfileLifecycleNative,
} from "./workspace-profile-lifecycle";
import type { ProfileKind } from "./workspace-profiles";

const label = (kind: ProfileKind) => (kind === "sh" ? "POSIX sh" : kind);
export class ProfileLifecycleForm {
  private editor: ProfileLifecycleEditor | undefined;
  private version = 0;
  private renderVersion = 0;
  private nonce: string | undefined;
  private busy = false;
  private expiry: ReturnType<typeof setTimeout> | undefined;
  private reading: AbortController | undefined;
  constructor(
    private readonly options: {
      root: HTMLElement;
      signal: AbortSignal;
      nonce(): string | undefined;
      native: ProfileLifecycleNative;
      engaged(value: boolean): void;
      changed(): Promise<void>;
    },
  ) {
    options.signal.addEventListener("abort", this.close, { once: true });
  }
  close = (): void => {
    ++this.version;
    clearTimeout(this.expiry);
    this.reading?.abort();
    this.editor?.close();
    this.editor = undefined;
    this.busy = false;
    this.options.root.replaceChildren();
  };
  private current(version: number): boolean {
    return (
      version === this.version &&
      !this.options.signal.aborted &&
      this.nonce === this.options.nonce()
    );
  }
  async open(message: string | UiMessage = ""): Promise<void> {
    this.close();
    const nonce = this.options.nonce();
    if (!nonce || this.options.signal.aborted) return;
    this.nonce = nonce;
    const version = this.version;
    const reading = new AbortController();
    this.reading = reading;
    this.render(null, "正在读取已删除 Profile…");
    const value = await new Promise<readonly ProfileKind[] | null>(
      (resolve) => {
        let done = false;
        const finish = (value: readonly ProfileKind[] | null) => {
          if (done) return;
          done = true;
          clearTimeout(timer);
          reading.signal.removeEventListener("abort", abort);
          resolve(value);
        };
        const abort = () => finish(null);
        const timer = setTimeout(abort, 5000);
        reading.signal.addEventListener("abort", abort, { once: true });
        try {
          void this.options.native.deleted(nonce).then((value) => {
            try {
              finish(decodeDeletedProfiles(value));
            } catch {
              finish(null);
            }
          }, abort);
        } catch {
          abort();
        }
      },
    );
    if (this.current(version))
      this.render(
        null,
        value ? message : "无法读取已删除项，请重新读取。",
        value ?? [],
      );
  }
  async prepare(shell: ProfileKind, action: LifecycleAction): Promise<void> {
    if (this.busy || this.editor || this.options.signal.aborted) return;
    this.close();
    const nonce = this.options.nonce();
    if (!nonce) return;
    this.nonce = nonce;
    const version = this.version;
    this.editor = new ProfileLifecycleEditor(
      nonce,
      this.options.signal,
      this.options.native,
    );
    this.busy = true;
    this.options.engaged(true);
    this.render(null, "正在确认 Profile…");
    const editor = this.editor;
    const offer = await editor.prepare(shell, action);
    if (!this.current(version)) {
      editor.close();
      return;
    }
    this.busy = false;
    if (!offer) {
      this.editor.close();
      this.editor = undefined;
      this.options.engaged(false);
      this.render(null, "无法准备操作，请重新读取后再确认。");
      return;
    }
    this.render(offer, "");
    this.expiry = setTimeout(() => {
      if (!this.current(version) || this.busy) return;
      this.editor?.close();
      this.editor = undefined;
      this.render(null, "确认已过期，请重新读取。");
      this.options.engaged(false);
    }, 55000);
  }
  private render(
    offer: LifecycleOffer | null,
    message: string | UiMessage,
    deleted: readonly ProfileKind[] = [],
  ): void {
    const root = this.options.root,
      document = root.ownerDocument,
      version = this.version,
      rendered = ++this.renderVersion;
    const current = () =>
      this.current(version) && rendered === this.renderVersion;
    const heading = document.createElement("h3");
    uiText(
      heading,
      offer
        ? uiMessage(
            offer.action === "delete" ? "删除 {0}" : "恢复 {0}",
            label(offer.shell),
          )
        : "已删除 Profile",
    );
    const status = document.createElement("p");
    status.setAttribute("role", "status");
    status.setAttribute("aria-live", "polite");
    uiText(
      status,
      message ||
        (offer
          ? offer.action === "delete"
            ? offer.isDefault
              ? "删除后不再显示或发现此项，已有终端继续运行。它仍是默认 Profile，删除后默认启动不可用。"
              : "删除后不再显示或发现此项，已有终端继续运行。"
            : offer.isDefault
              ? "恢复后按系统发现结果显示，不会启动终端。它仍是已保存的默认 Profile。"
              : "恢复后按系统发现结果显示，不会启动终端。"
          : deleted.length
            ? "恢复后不会启动终端。"
            : "没有已删除项。"),
    );
    const actions = document.createElement("div");
    actions.className = "terminal-paste-actions";
    const button = (
      text: string | UiMessage,
      action: () => void,
      disabled = false,
    ) => {
      const node = document.createElement("button");
      node.type = "button";
      uiText(node, text);
      node.disabled = disabled;
      node.addEventListener(
        "click",
        () => {
          if (current() && !node.disabled) action();
        },
        { signal: this.options.signal },
      );
      actions.append(node);
      return node;
    };
    if (offer) {
      button("取消", () => void this.cancelAndRead());
      button(
        offer.action === "delete" ? "确认删除" : "确认恢复",
        () => void this.commit(offer),
        this.busy,
      );
    } else if (!this.busy) {
      for (const shell of deleted)
        button(
          uiMessage("恢复 {0}", label(shell)),
          () => void this.prepare(shell, "restore"),
        );
      button("重新读取已删除项", () => void this.open());
    } else {
      button("取消操作", () => void this.cancelAndRead());
    }
    root.replaceChildren(heading, status, actions);
    if (offer && !this.busy)
      actions.querySelector<HTMLButtonElement>("button")?.focus();
  }
  private async cancelAndRead(): Promise<void> {
    const reopening = this.version + 1;
    await this.open();
    if (this.current(reopening)) this.options.engaged(false);
  }
  private async commit(offer: LifecycleOffer): Promise<void> {
    if (this.busy || !this.editor) return;
    const version = this.version;
    const editor = this.editor;
    this.busy = true;
    clearTimeout(this.expiry);
    this.render(offer, "正在保存…");
    const result = await editor.commit();
    if (!this.current(version)) {
      editor.close();
      return;
    }
    this.busy = false;
    editor.close();
    this.editor = undefined;
    if (result !== "saved") {
      this.render(null, "结果未确认，请重新读取检查，不自动重试。");
      this.options.engaged(false);
      return;
    }
    try {
      await this.options.changed();
      if (!this.current(version)) return;
      const reopening = this.version + 1;
      await this.open(
        offer.action === "delete"
          ? "Profile 已删除，已有终端继续运行。"
          : "Profile 已恢复，未启动终端。",
      );
      if (!this.current(reopening)) return;
      this.options.engaged(false);
    } catch {
      if (this.current(version)) {
        this.render(null, "保存已完成，但无法读取当前列表，请重新读取。");
        this.options.engaged(false);
      }
    }
  }
}
