import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import type { NativeVisibilitySample } from "../../ipc/gen/commands";
import {
  NativeVisibilityBuffer,
  NativeVisibilityFilter,
  nativeShown,
} from "./native-visibility";

export interface VisibilityDependencies {
  listen(
    receive: (sample: NativeVisibilitySample) => void,
  ): Promise<() => void>;
  snapshot(nonce: string): Promise<NativeVisibilitySample>;
}
const native: VisibilityDependencies = {
  listen: async (receive) =>
    getCurrentWindow().listen<NativeVisibilitySample>(
      "workspace-native-visibility",
      (event) => receive(event.payload),
    ),
  snapshot: (nonce) => invoke("workspace_native_visibility", { nonce }),
};
type Subscriber = {
  changed(visible: boolean): void;
  failed(error: Error): void;
};

/** One native listener per document/page, independent of the number of split panes. */
export class WorkspaceVisibility {
  readonly ready: Promise<void>;
  private resolve!: () => void;
  private reject!: (error: Error) => void;
  private listeners = new Set<Subscriber>();
  private filter: NativeVisibilityFilter | undefined;
  private buffer = new NativeVisibilityBuffer();
  private shown = false;
  private stopped = false;
  private error: Error | undefined;
  private unlisten: (() => void) | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly changed = () => this.publish();
  constructor(
    private readonly document: Pick<
      Document,
      "visibilityState" | "addEventListener" | "removeEventListener"
    >,
    nonce: string,
    dependencies: VisibilityDependencies = native,
    private readonly empty: () => void = () => {},
  ) {
    this.ready = new Promise<void>((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
    });
    void this.ready.catch(() => {});
    this.document.addEventListener("visibilitychange", this.changed);
    this.timer = setTimeout(
      () => this.fail(Error("workspace.visibility_timeout")),
      2000,
    );
    void (async () => {
      const stop = await dependencies.listen((sample) => this.receive(sample));
      if (this.stopped) {
        stop();
        return;
      }
      this.unlisten = stop;
      const snapshot = await dependencies.snapshot(nonce);
      if (this.stopped) return;
      if (!snapshot.valid) throw Error("workspace.visibility_failed");
      this.filter = new NativeVisibilityFilter(snapshot);
      this.shown = nativeShown(snapshot);
      const pending = this.buffer.take(snapshot.incarnation);
      if (pending) this.apply(pending);
      if (this.stopped) return;
      clearTimeout(this.timer);
      this.publish();
      this.resolve();
    })().catch(() => this.fail(Error("workspace.visibility_failed")));
  }
  subscribe(
    changed: Subscriber["changed"],
    failed: Subscriber["failed"],
  ): () => void {
    const subscriber = { changed, failed };
    this.listeners.add(subscriber);
    this.notify(subscriber);
    if (this.error) this.feedback(() => failed(this.error as Error));
    return () => {
      this.listeners.delete(subscriber);
      if (!this.listeners.size) {
        this.dispose();
        this.empty();
      }
    };
  }
  private feedback(action: () => void): void {
    try {
      action();
    } catch {
      /* Feedback cannot grant authority. */
    }
  }
  private notify(subscriber: Subscriber): void {
    this.feedback(() =>
      subscriber.changed(
        !this.stopped &&
          this.shown &&
          this.document.visibilityState === "visible",
      ),
    );
  }
  private publish(): void {
    for (const subscriber of this.listeners) this.notify(subscriber);
  }
  private receive(sample: NativeVisibilitySample): void {
    if (this.stopped) return;
    try {
      if (this.filter) {
        this.apply(sample);
        this.publish();
      } else this.buffer.add(sample);
    } catch {
      this.fail(Error("workspace.invalid_visibility"));
    }
  }
  private apply(sample: NativeVisibilitySample): void {
    const accepted = this.filter?.accept(sample);
    if (!accepted) return;
    if (!accepted.valid) {
      this.fail(Error("workspace.visibility_failed"));
      return;
    }
    this.shown = nativeShown(accepted);
  }
  private fail(error: Error): void {
    if (this.stopped) return;
    this.error = error;
    this.dispose();
    for (const subscriber of this.listeners) {
      this.notify(subscriber);
      this.feedback(() => subscriber.failed(error));
    }
  }
  dispose(): void {
    if (this.stopped) return;
    this.stopped = true;
    this.publish();
    clearTimeout(this.timer);
    this.document.removeEventListener("visibilitychange", this.changed);
    const stop = this.unlisten;
    this.unlisten = undefined;
    if (stop) this.feedback(stop);
    this.reject(this.error ?? Error("workspace.retired"));
  }
}
const sources = new WeakMap<
  Document,
  { nonce: string; source: WorkspaceVisibility }
>();
function source(document: Document, nonce: string): WorkspaceVisibility {
  const current = sources.get(document);
  if (current?.nonce === nonce) return current.source;
  if (current) current.source.dispose();
  const next = new WorkspaceVisibility(document, nonce, native, () => {
    if (sources.get(document)?.source === next) sources.delete(document);
  });
  sources.set(document, { nonce, source: next });
  return next;
}
export function observeWorkspaceVisibility(
  document: Document,
  nonce: string,
  changed: Subscriber["changed"],
  failed: Subscriber["failed"],
): () => void {
  return source(document, nonce).subscribe(changed, failed);
}
/** Bootstrap before creating any pane, so a visible first pane can mount normally. */
export async function prepareWorkspaceVisibility(
  document: Document,
  nonce: string,
  signal: AbortSignal,
): Promise<void> {
  if (signal.aborted) throw Error("workspace.retired");
  const visibility = source(document, nonce);
  const stop = visibility.subscribe(
    () => {},
    () => {},
  );
  const retire = () => {
    stop();
    signal.removeEventListener("abort", retire);
  };
  signal.addEventListener("abort", retire, { once: true });
  try {
    await visibility.ready;
    if (signal.aborted) throw Error("workspace.retired");
  } catch (error) {
    retire();
    throw error;
  }
}
