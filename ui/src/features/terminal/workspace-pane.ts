import { GenerationViewHost } from "./view-host";
import { WorkspaceFit } from "./workspace-fit";
import {
  prepareWorkspaceTerminal,
  type TerminalConnectionOptions,
  TerminalDetachFailed,
  TerminalLayoutChanged,
  type WorkspaceTerminalConnection,
} from "./workspace-terminal-connection";
import { observeWorkspaceVisibility } from "./workspace-visibility";
export type PanePhase =
  | "loading"
  | "ready"
  | "resizing"
  | "paused"
  | "ended"
  | "failed"
  | "disposed";
type Connection = Pick<
  WorkspaceTerminalConnection,
  | "ready"
  | "dispose"
  | "suspend"
  | "geometry"
  | "measuredDimensions"
  | "pauseForResize"
  | "resize"
  | "watchGeometry"
  | "announceInputReady"
  | "focusOutput"
>;
export interface PaneDependencies {
  prepare(options: TerminalConnectionOptions): Promise<Connection>;
  fit(
    view: Connection,
    callbacks: {
      paused(): void;
      replaced(): void;
      failed(error: Error): void;
      settled(): void;
    },
    signal: AbortSignal,
  ): { observe(): void; dispose(): void };
  observe(element: HTMLElement, notify: () => void): () => void;
  observeVisibility?(
    element: HTMLElement,
    notify: (visible: boolean) => void,
    failed: (error: Error) => void,
    nonce: string,
  ): () => void;
}
const native: PaneDependencies = {
  prepare: prepareWorkspaceTerminal,
  fit: (view, callbacks, signal) => new WorkspaceFit(view, callbacks, signal),
  observe: (element, notify) => {
    const observer = new ResizeObserver(notify);
    observer.observe(element);
    return () => observer.disconnect();
  },
  observeVisibility: (element, notify, failed, nonce) =>
    observeWorkspaceVisibility(element.ownerDocument, nonce, notify, failed),
};
/** Owns a disposable view, not its session. Hidden views release rendering and delivery. */
export class WorkspacePane {
  private readonly life = new AbortController();
  private readonly host: GenerationViewHost;
  private view: Connection | undefined;
  private stopFit: (() => void) | undefined;
  private stopVisibility: (() => void) | undefined;
  private task: Promise<void> | undefined;
  private pendingPrepare: Promise<Connection> | undefined;
  private releasing: Promise<void> | undefined;
  private attempt: AbortController | undefined;
  private phase: PanePhase = "loading";
  private disposed = false;
  private blocked = false;
  private requested = false;
  private running = false;
  private layoutVisible = true;
  private pageVisible = true;
  private epoch = 0;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly options: Omit<
      TerminalConnectionOptions,
      "host" | "failed" | "observed" | "signal"
    > & { signal: AbortSignal; phase(phase: PanePhase, error?: Error): void },
    private readonly dependencies: PaneDependencies = native,
  ) {
    this.host = new GenerationViewHost(this.life.signal, (error) =>
      this.failed(error),
    );
    options.signal.addEventListener("abort", this.abort, { once: true });
    if (options.signal.aborted) this.dispose();
    else
      this.stopVisibility = dependencies.observeVisibility?.(
        options.element,
        (visible) => {
          void this.visibility(this.layoutVisible, visible).catch(() => {});
        },
        (error) => this.failed(error),
        options.nonce,
      );
  }
  get state(): PanePhase {
    return this.phase;
  }
  private get visible(): boolean {
    return this.layoutVisible && this.pageVisible;
  }
  focusOutput(): boolean {
    if (
      this.disposed ||
      !this.visible ||
      (this.phase !== "ready" && this.phase !== "ended")
    )
      return false;
    return this.view?.focusOutput() ?? false;
  }
  private publish(phase: PanePhase, error?: Error) {
    if (this.disposed && phase !== "disposed") return;
    if (this.blocked && phase !== "failed" && phase !== "disposed") return;
    this.phase = phase;
    try {
      this.options.phase(phase, error);
    } catch {
      /* Feedback grants no authority. */
    }
  }
  mount(): Promise<void> {
    if (this.disposed) return Promise.reject(Error("view.retired"));
    this.requested = true;
    return this.schedule();
  }
  /** Layout visibility is independent of the document's own visibility. */
  setVisible(visible: boolean): Promise<void> {
    return this.visibility(visible, this.pageVisible);
  }
  private visibility(layout: boolean, page: boolean): Promise<void> {
    const wasVisible = this.visible;
    this.layoutVisible = layout;
    this.pageVisible = page;
    if (
      this.disposed ||
      this.blocked ||
      !this.requested ||
      wasVisible === this.visible
    )
      return Promise.resolve();
    if (!this.visible) {
      this.retire();
      this.publish("paused");
    }
    return this.schedule();
  }
  private schedule(): Promise<void> {
    if (this.disposed) return Promise.reject(Error("view.retired"));
    if (this.blocked)
      return Promise.reject(Error("terminal.connection_failed"));
    if (this.running || this.view) return this.task ?? Promise.resolve();
    this.running = true;
    this.task = this.attach()
      .catch((error: unknown) => {
        if (!this.disposed)
          this.failed(
            error instanceof Error
              ? error
              : Error("terminal.connection_failed"),
          );
        throw error;
      })
      .finally(() => {
        this.running = false;
        if (
          !this.disposed &&
          !this.blocked &&
          this.requested &&
          this.visible &&
          !this.view
        )
          return this.schedule();
      });
    void this.task.catch(() => {});
    return this.task;
  }
  private release(view: Connection): Promise<void> {
    try {
      return view.suspend().catch(() => {
        throw new TerminalDetachFailed();
      });
    } catch {
      return Promise.reject(new TerminalDetachFailed());
    }
  }
  private retire(): void {
    this.epoch++;
    this.stopFit?.();
    this.stopFit = undefined;
    const view = this.view,
      preparing = this.pendingPrepare;
    this.view = undefined;
    const work: Promise<void>[] = [];
    if (this.releasing) work.push(this.releasing);
    if (view) work.push(this.release(view));
    if (preparing)
      work.push(
        preparing.then(
          (late) => this.release(late),
          (error: unknown) => {
            if (error instanceof TerminalDetachFailed) throw error;
          },
        ),
      );
    this.attempt?.abort();
    this.attempt = undefined;
    if (!view && !preparing) return;
    this.releasing = new Promise<void>((resolve, reject) => {
      const timer = setTimeout(() => reject(new TerminalDetachFailed()), 1500);
      void Promise.all(work).then(
        () => {
          clearTimeout(timer);
          resolve();
        },
        () => {
          clearTimeout(timer);
          reject(new TerminalDetachFailed());
        },
      );
    });
    void this.releasing.catch((error: Error) => this.failed(error));
  }
  private async attach(): Promise<void> {
    while (!this.disposed && !this.blocked) {
      const releasing = this.releasing;
      if (releasing) {
        await releasing;
        if (this.releasing === releasing) this.releasing = undefined;
      }
      if (this.disposed || this.blocked) return;
      if (!this.visible) {
        this.publish("paused");
        return;
      }
      this.publish("loading");
      const epoch = ++this.epoch,
        attempt = new AbortController();
      this.attempt = attempt;
      const current = () =>
        !this.disposed && !this.blocked && this.visible && epoch === this.epoch;
      let next: Connection | undefined;
      try {
        const preparing = this.dependencies.prepare({
          ...this.options,
          host: this.host,
          signal: attempt.signal,
          failed: (error) => {
            if (current()) this.failed(error);
          },
          observed: (boundary) => {
            if (boundary.kind === "end" && current()) {
              this.stopFit?.();
              this.stopFit = undefined;
              this.publish("ended");
            }
          },
        });
        this.pendingPrepare = preparing;
        try {
          next = await preparing;
        } finally {
          if (this.pendingPrepare === preparing)
            this.pendingPrepare = undefined;
        }
        if (!current()) {
          await this.release(next);
          continue;
        }
        this.view = next;
        try {
          await next.ready;
        } catch (error) {
          if (error instanceof TerminalLayoutChanged && current()) {
            this.view = undefined;
            await this.release(next);
            continue;
          }
          throw error;
        }
        if (!current() || this.view !== next) {
          await this.release(next);
          continue;
        }
        if (this.phase === "ended") return;
        this.publish("ready");
        if (!current()) continue;
        this.watch(next);
        if (current() && this.view === next) return;
      } catch (error) {
        if (
          !current() &&
          !this.blocked &&
          !(error instanceof TerminalDetachFailed)
        )
          continue;
        throw error;
      } finally {
        if (this.view !== next || !current()) {
          attempt.abort();
          if (this.attempt === attempt) this.attempt = undefined;
        }
      }
    }
    if (this.blocked && !this.disposed)
      throw Error("terminal.connection_failed");
  }
  private watch(view: Connection): void {
    const life = new AbortController(),
      current = () =>
        !this.disposed && !life.signal.aborted && this.view === view;
    const fit = this.dependencies.fit(
      view,
      {
        paused: () => {
          if (current()) this.publish("resizing");
        },
        replaced: () => {
          if (!current()) return;
          this.retire();
          this.publish("loading");
          void this.schedule().catch(() => {});
        },
        failed: (error) => {
          if (current()) this.failed(error);
        },
        settled: () => {
          if (!current()) return;
          this.publish("ready");
          void view.announceInputReady().catch((error: unknown) => {
            if (current())
              this.failed(
                error instanceof Error ? error : Error("input.uncertain"),
              );
          });
        },
      },
      life.signal,
    );
    const stopObserve = this.dependencies.observe(this.options.element, () => {
      if (current()) fit.observe();
    });
    this.stopFit = () => {
      stopObserve();
      life.abort();
      fit.dispose();
    };
    fit.observe();
  }
  private failed(error: Error): void {
    if (this.disposed || this.blocked) return;
    this.blocked = true;
    this.epoch++;
    this.stopFit?.();
    this.stopFit = undefined;
    this.attempt?.abort();
    this.attempt = undefined;
    this.view?.dispose();
    this.view = undefined;
    this.publish("failed", error);
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.epoch++;
    this.options.signal.removeEventListener("abort", this.abort);
    this.stopVisibility?.();
    this.stopFit?.();
    this.stopFit = undefined;
    this.attempt?.abort();
    this.life.abort();
    this.view?.dispose();
    this.view = undefined;
    this.publish("disposed");
  }
}
