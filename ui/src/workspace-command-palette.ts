import { CompositionGuard, isCompositionKey } from "./keybindings/composition";
import {
  captureWorkspaceCommands,
  isCommandPaletteShortcut,
  type WorkspaceCommand,
  type WorkspaceCommandCapture,
  type WorkspaceCommandContext,
  type WorkspaceCommandId,
} from "./workspace-command-catalog";
import { message, uiText } from "./workspace-language";
export interface WorkspaceCommandPaletteOptions {
  readonly dialog: HTMLDialogElement;
  readonly search: HTMLInputElement;
  readonly list: HTMLElement;
  readonly status: HTMLElement;
  readonly done: HTMLButtonElement;
  readonly context: WorkspaceCommandContext;
  readonly commands: readonly WorkspaceCommand[];
}
/** A current-page navigation chooser. Native action admission stays in existing services. */
export class WorkspaceCommandPalette {
  private capture: WorkspaceCommandCapture | undefined;
  private nonce: string | undefined;
  private opener: HTMLElement | null = null;
  private items: readonly Pick<WorkspaceCommand, "id" | "title">[] = [];
  private selected = 0;
  private renderRevision = 0;
  constructor(private readonly options: WorkspaceCommandPaletteOptions) {
    const listeners = { signal: options.context.signal };
    const composition = new CompositionGuard(
      options.search,
      options.context.signal,
    );
    options.search.addEventListener("input", () => this.render(), listeners);
    options.search.addEventListener(
      "keydown",
      (event) => {
        if (composition.handles(event)) return;
        if (event.key === "ArrowDown" || event.key === "ArrowUp") {
          event.preventDefault();
          if (this.items.length) {
            this.selected =
              (this.selected +
                (event.key === "ArrowDown" ? 1 : -1) +
                this.items.length) %
              this.items.length;
            this.mark();
          }
        } else if (event.key === "Enter") {
          event.preventDefault();
          if (!event.repeat) {
            const item = this.items[this.selected];
            if (item) this.choose(item.id);
          }
        }
      },
      listeners,
    );
    options.done.addEventListener("click", () => this.close(), listeners);
    options.dialog.addEventListener(
      "cancel",
      (event) => {
        event.preventDefault();
        this.close();
      },
      listeners,
    );
    options.dialog.addEventListener(
      "close",
      () => {
        if (!options.dialog.open && this.capture) this.close();
      },
      listeners,
    );
    options.context.signal.addEventListener("abort", () => this.close(false), {
      once: true,
    });
  }
  /** Consume repeats too, so a held shortcut never becomes PTY Ctrl+P. */
  shortcut(event: KeyboardEvent): boolean {
    if (isCompositionKey(event)) return false;
    if (
      !isCommandPaletteShortcut({
        key: event.key,
        ctrlKey: event.ctrlKey,
        metaKey: event.metaKey,
        shiftKey: event.shiftKey,
        altKey: event.altKey,
        repeat: false,
        isComposing: event.isComposing,
        defaultPrevented: event.defaultPrevented,
      })
    )
      return false;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (!event.repeat) this.open();
    return true;
  }
  open(): boolean {
    if (this.options.dialog.open || this.options.context.signal.aborted)
      return false;
    const capture = captureWorkspaceCommands(
      this.options.context,
      this.options.commands,
    );
    if (!capture) return false;
    this.capture = capture;
    this.nonce = this.options.context.nonce();
    this.opener = this.options.dialog.ownerDocument
      .activeElement as HTMLElement | null;
    this.options.search.value = "";
    this.selected = 0;
    this.options.search.setAttribute("aria-expanded", "true");
    this.render();
    try {
      this.options.dialog.showModal();
      this.options.search.focus();
      return true;
    } catch {
      this.close(false);
      return false;
    }
  }
  /** Called when fit/busy controls change; never leave a stale choice visible. */
  check(): void {
    if (
      this.capture &&
      (!this.options.context.available() ||
        this.options.context.signal.aborted ||
        this.nonce !== this.options.context.nonce())
    )
      this.close(false);
  }
  private render(): void {
    if (!this.capture) return;
    this.check();
    if (!this.capture) return;
    const capture = this.capture;
    const revision = ++this.renderRevision;
    this.items = capture.list(this.options.search.value);
    this.selected = 0;
    this.options.list.replaceChildren();
    for (const item of this.items) {
      const option = this.options.dialog.ownerDocument.createElement("li");
      option.id = `workspace-command-${item.id}`;
      option.setAttribute("role", "option");
      uiText(option, item.title);
      option.addEventListener("click", () => {
        if (this.capture === capture && this.renderRevision === revision)
          this.choose(item.id);
      });
      this.options.list.append(option);
    }
    uiText(
      this.options.status,
      this.items.length
        ? message("{0} 个可用命令", this.items.length)
        : "没有匹配的可用命令",
    );
    this.mark();
  }
  private mark(): void {
    const current = this.items[this.selected];
    if (current)
      this.options.search.setAttribute(
        "aria-activedescendant",
        `workspace-command-${current.id}`,
      );
    else this.options.search.removeAttribute("aria-activedescendant");
    Array.from(this.options.list.children).forEach((option, index) => {
      option.setAttribute("aria-selected", String(index === this.selected));
      if (index === this.selected)
        option.scrollIntoView?.({ block: "nearest" });
    });
  }
  private choose(id: WorkspaceCommandId): void {
    const capture = this.capture;
    if (!capture) return;
    if (
      !capture.list(this.options.search.value).some((item) => item.id === id)
    ) {
      this.close(false);
      return;
    }
    // Close first; restore the current opener so a child editor can return there.
    // execute still rechecks nonce/busy/target after focus and consumes once.
    this.detach(true);
    try {
      capture.execute(id);
    } finally {
      capture.close();
    }
  }
  private detach(restore: boolean): void {
    const nonce = this.nonce,
      opener = this.opener;
    this.capture = undefined;
    this.nonce = undefined;
    this.opener = null;
    this.items = [];
    this.options.search.value = "";
    this.options.search.setAttribute("aria-expanded", "false");
    this.options.search.removeAttribute("aria-activedescendant");
    this.options.list.replaceChildren();
    uiText(this.options.status, "");
    if (this.options.dialog.open) this.options.dialog.close();
    if (
      restore &&
      !this.options.context.signal.aborted &&
      this.options.context.nonce() === nonce &&
      this.options.context.available() &&
      opener?.isConnected
    )
      opener.focus();
  }
  close(restore = true): void {
    const capture = this.capture;
    if (!capture) return;
    this.detach(restore);
    capture.close();
  }
}
