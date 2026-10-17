import { FitScheduler, type Geometry } from "./fit-scheduler";
import type { WorkspaceTerminalConnection } from "./workspace-terminal-connection";
export type FitConnection = Pick<
  WorkspaceTerminalConnection,
  "geometry" | "measuredDimensions" | "pauseForResize" | "resize"
> &
  Partial<Pick<WorkspaceTerminalConnection, "watchGeometry">>;
/** One backend Begin generation. Metadata measurements never authorize new views.
 * Observer storms keep only latest dimensions; native errors are not retried. */
export class WorkspaceFit {
  private readonly fit: FitScheduler;
  private readonly current: Readonly<Geometry>;
  private closed = false;
  private paused = false;
  private started = false;
  private settled = false;
  private readonly stopGeometry: (() => void) | undefined;
  constructor(
    private readonly view: FitConnection,
    private readonly callbacks: {
      paused(): void;
      replaced(): void;
      failed(error: Error): void;
      settled(): void;
    },
    private readonly signal: AbortSignal,
  ) {
    this.current = view.geometry();
    this.fit = new FitScheduler(
      this.current,
      async (size) => {
        this.started = true;
        await view.resize(size);
      },
      () => {
        if (!this.closed && !signal.aborted) {
          this.dispose();
          callbacks.replaced();
        }
      },
      (error) => {
        if (!this.closed && !signal.aborted) {
          this.dispose();
          callbacks.failed(error);
        }
      },
      signal,
    );
    this.stopGeometry = view.watchGeometry?.(() => this.observe());
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  private readonly abort = () => this.dispose();
  observe(): void {
    if (this.closed || this.signal.aborted) return;
    try {
      const size = this.view.measuredDimensions();
      if (!size) {
        this.fit.observe(undefined);
        return;
      }
      if (
        size.columns === this.current.columns &&
        size.rows === this.current.rows
      ) {
        if (this.paused && !this.started) {
          this.dispose();
          this.callbacks.replaced();
          return;
        }
        this.fit.observe(size);
        if (!this.paused && !this.settled) {
          this.settled = true;
          this.callbacks.settled();
        }
        return;
      }
      if (!this.paused) {
        this.paused = true;
        this.view.pauseForResize();
        this.callbacks.paused();
      }
      this.fit.observe(size);
    } catch (error) {
      this.dispose();
      this.callbacks.failed(
        error instanceof Error ? error : new Error("fit.measurement"),
      );
    }
  }
  dispose(): void {
    if (this.closed) return;
    this.closed = true;
    this.stopGeometry?.();
    this.signal.removeEventListener("abort", this.abort);
    this.fit.dispose();
  }
}
