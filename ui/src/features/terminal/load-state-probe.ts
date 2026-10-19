import { invoke } from "@tauri-apps/api/core";
import type { DrawnBoundary } from "./terminal-view-stream";

type ProbeKey = Pick<
  KeyboardEvent,
  | "target"
  | "isTrusted"
  | "code"
  | "ctrlKey"
  | "shiftKey"
  | "altKey"
  | "metaKey"
  | "repeat"
  | "isComposing"
  | "preventDefault"
  | "stopImmediatePropagation"
>;

/** Fixed, physical-key trigger in the separate load verification bundle only. */
export class LoadStateProbe {
  private closed = false;
  private requested = false;
  private comparing = false;
  constructor(
    private readonly options: {
      element: HTMLElement;
      signal: AbortSignal;
      isCurrent(): boolean;
      nonce: string;
      connection: string;
      failed(error: Error): void;
    },
  ) {
    options.element.addEventListener("keydown", this.key, true);
    options.signal.addEventListener("abort", this.dispose, { once: true });
    if (options.signal.aborted) this.dispose();
  }
  private live(): boolean {
    return (
      !this.closed && !this.options.signal.aborted && this.options.isCurrent()
    );
  }
  readonly key = async (event: ProbeKey): Promise<void> => {
    if (
      !this.live() ||
      this.requested ||
      this.comparing ||
      !event.isTrusted ||
      event.code !== "F8" ||
      !event.ctrlKey ||
      !event.shiftKey ||
      event.altKey ||
      event.metaKey ||
      event.repeat ||
      event.isComposing ||
      !(event.target instanceof Node) ||
      !this.options.element.contains(event.target)
    )
      return;
    event.preventDefault();
    event.stopImmediatePropagation();
    this.requested = true;
    try {
      await invoke("workspace_terminal_load_marker", {
        nonce: this.options.nonce,
        connection: this.options.connection,
      });
    } catch {
      this.requested = false;
      if (this.live())
        this.options.failed(new Error("terminal.load_marker_failed"));
    }
  };
  async observe(boundary: Readonly<DrawnBoundary>): Promise<void> {
    if (
      !this.live() ||
      !this.requested ||
      this.comparing ||
      boundary.kind !== "marker"
    )
      return;
    this.requested = false;
    this.comparing = true;
    try {
      await invoke("workspace_terminal_load_compare", {
        nonce: this.options.nonce,
        connection: this.options.connection,
        rendered: {
          columns: boundary.columns,
          rows: boundary.rows,
          revision: boundary.revision.toString(),
          sourceBytes: boundary.sourceBytes.toString(),
          utf8Bytes: boundary.utf8Bytes.toString(),
          digest: boundary.digest,
        },
      });
    } finally {
      this.comparing = false;
    }
  }
  readonly dispose = (): void => {
    if (this.closed) return;
    this.closed = true;
    this.requested = false;
    this.options.element.removeEventListener("keydown", this.key, true);
    this.options.signal.removeEventListener("abort", this.dispose);
  };
}
