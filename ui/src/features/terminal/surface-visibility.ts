export type VisibilitySource = "container" | "document";
export interface VisibilityActions {
  /** Synchronous: cancel input/render/ACK work BEFORE any asynchronous detach. */
  retire(): void;
  pause(): Promise<void>;
  /** Before invoking IPC, authorize precisely one new Begin. No optimistic render. */
  resume(): Promise<void>;
  failed(error: Error): void;
}
interface Completion {
  promise: Promise<void>;
  resolve: () => void;
  reject: (error: Error) => void;
}
function completion(): Completion {
  let resolve!: () => void, reject!: (error: Error) => void;
  const promise = new Promise<void>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  // Owners may dispose before awaiting either phase. Still reject to actual waiters.
  void promise.catch(() => {});
  return { promise, resolve, reject };
}
/** One disposable viewer generation. Hidden sources combine with AND; losing focus
 * is NOT a hidden signal. Pause cannot be skipped even when show arrives early.
 * The new generation owns the next controller after a successful resume handoff. */
export class SurfaceVisibility {
  private sources = { container: true, document: true };
  private phase:
    | "visible"
    | "pausing"
    | "paused"
    | "resuming"
    | "handedoff"
    | "closed" = "visible";
  private deadline: ReturnType<typeof setTimeout> | undefined;
  private pausedCompletion = completion();
  private resumedCompletion = completion();
  readonly paused = this.pausedCompletion.promise;
  readonly resumed = this.resumedCompletion.promise;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly actions: VisibilityActions,
    private readonly signal: AbortSignal,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  observe(source: VisibilitySource, visible: boolean): void {
    this.observeSnapshot({ ...this.sources, [source]: visible });
  }
  /** Commit one measurement atomically: source updates must not create a
   * transient visible state between a container show and page hide. */
  observeSnapshot(sources: Record<VisibilitySource, boolean>): void {
    if (this.phase === "closed" || this.phase === "handedoff") return;
    this.sources = { ...sources };
    const shown = this.sources.container && this.sources.document;
    if (!shown && this.phase === "resuming") {
      this.fail("visibility.changed_during_resume");
      return;
    }
    if (!shown && this.phase === "visible") {
      this.phase = "pausing";
      try {
        this.actions.retire();
      } catch {
        this.fail("visibility.retire_failed");
        return;
      }
      if (this.phase !== "pausing" || this.signal.aborted) return;
      this.run(
        "pausing",
        () => this.actions.pause(),
        () => {
          this.phase = "paused";
          this.pausedCompletion.resolve();
          this.maybeResume();
        },
      );
    } else if (shown) this.maybeResume();
  }
  dispose(): void {
    this.close(new Error("visibility.cancelled"));
  }
  private close(error: Error): void {
    if (this.phase === "closed" || this.phase === "handedoff") return;
    this.phase = "closed";
    clearTimeout(this.deadline);
    this.signal.removeEventListener("abort", this.abort);
    this.pausedCompletion.reject(error);
    this.resumedCompletion.reject(error);
  }
  private fail(message: string): void {
    if (this.phase === "closed" || this.phase === "handedoff") return;
    const error = new Error(message);
    this.close(error);
    this.actions.failed(error);
  }
  private maybeResume(): void {
    if (
      this.phase !== "paused" ||
      !this.sources.container ||
      !this.sources.document
    )
      return;
    this.phase = "resuming";
    this.run(
      "resuming",
      () => this.actions.resume(),
      () => {
        this.phase = "handedoff";
        this.signal.removeEventListener("abort", this.abort);
        this.resumedCompletion.resolve();
      },
    );
  }
  private run(
    phase: "pausing" | "resuming",
    action: () => Promise<void>,
    done: () => void,
  ): void {
    this.deadline = setTimeout(() => this.fail("visibility.uncertain"), 1500);
    // Defer only the IPC invocation, never local retirement. Recheck after disposal.
    void Promise.resolve()
      .then(() => (this.phase === phase ? action() : undefined))
      .then(() => {
        if (this.phase !== phase || this.signal.aborted) return;
        clearTimeout(this.deadline);
        done();
      })
      .catch(() => {
        if (this.phase === phase) this.fail("visibility.control_failed");
      });
  }
}
/** Observe explicit host.hidden and real page visibility. This does not pretend
 * to detect arbitrary ancestor CSS occlusion, native minimize or loss of focus. */
export function bindSurfaceVisibility(
  host: HTMLElement,
  view: SurfaceVisibility,
  signal: AbortSignal,
): () => void {
  const document = host.ownerDocument;
  const measure = () => {
    view.observeSnapshot({
      container: host.isConnected && !host.hidden,
      document: document.visibilityState === "visible",
    });
  };
  const observer = new MutationObserver(measure);
  let disposed = false;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    observer.disconnect();
    document.removeEventListener("visibilitychange", measure);
    signal.removeEventListener("abort", dispose);
  };
  void view.resumed.then(dispose, dispose);
  void view.paused.catch(dispose);
  if (signal.aborted) {
    dispose();
    return dispose;
  }
  observer.observe(host, { attributes: true, attributeFilter: ["hidden"] });
  document.addEventListener("visibilitychange", measure);
  signal.addEventListener("abort", dispose, { once: true });
  measure();
  return dispose;
}
