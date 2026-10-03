import type { AttachBoundary } from "../../ipc/attachment";

export type VisibleComparison = "equal" | "mismatch" | "stale";
export interface IdleComparisonSink {
  /** Capture digest synchronously before awaiting IPC; abort forbids using late evidence. */
  compare(
    boundary: AttachBoundary,
    signal: AbortSignal,
  ): Promise<VisibleComparison>;
  /** Evidence only. A real recovery MUST revalidate generation and boundary atomically. */
  mismatch(boundary: AttachBoundary): void;
  matched?(boundary: AttachBoundary): void;
}
const IDLE_MS = 600;
const DEADLINE_MS = 350;
const MAX_ATTEMPTS = 3;
/** One visible generation, one finite idle check per known wire boundary, no polling.
 * Raw Data invalidates the known revision: do NOT infer one from byte counts.
 * A stalled IPC cannot spawn more IPCs. Replacing a generation requires dispose.
 */
export class IdleComparison {
  private boundary: AttachBoundary | undefined;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private active: AbortController | undefined;
  private epoch = 0;
  private attempts = 0;
  private visible = true;
  private disposed = false;
  private checked = false;
  constructor(private readonly sink: IdleComparisonSink) {}
  /** Call on admission of new output, BEFORE the renderer callback completes. */
  activity(): void {
    this.invalidate();
    this.boundary = undefined;
    this.checked = false;
    this.attempts = 0;
  }
  /** Call only after Commit/Checkpoint callback, with no later admitted Data. */
  observe(boundary: AttachBoundary): void {
    this.activity();
    this.boundary = { ...boundary };
    this.arm();
  }
  setVisible(visible: boolean): void {
    if (this.visible === visible || this.disposed) return;
    this.invalidate();
    this.visible = visible;
    this.checked = false;
    this.attempts = 0;
    this.arm();
  }
  dispose(): void {
    this.disposed = true;
    this.invalidate();
    this.boundary = undefined;
  }
  private invalidate(): void {
    this.epoch++;
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    this.active?.abort();
  }
  private arm(): void {
    if (
      this.disposed ||
      !this.visible ||
      !this.boundary ||
      this.active ||
      this.checked ||
      this.attempts >= MAX_ATTEMPTS
    )
      return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.check();
    }, IDLE_MS);
  }
  private async check(): Promise<void> {
    if (this.disposed || !this.visible || !this.boundary || this.active) return;
    const boundary = { ...this.boundary };
    const epoch = this.epoch;
    const controller = new AbortController();
    this.active = controller;
    this.attempts++;
    // Do not free the single-flight slot on deadline: non-cancellable IPC may
    // still be pending. It must settle before any further call can be admitted.
    const deadline = setTimeout(() => controller.abort(), DEADLINE_MS);
    try {
      const result = await this.sink.compare(boundary, controller.signal);
      if (this.epoch !== epoch || controller.signal.aborted || this.disposed)
        return;
      if (result !== "stale") {
        this.checked = true;
        if (result === "mismatch") this.sink.mismatch(boundary);
        else this.sink.matched?.(boundary);
      }
    } catch {
      // Bounded retry; Busy/closed/transport failures are NOT divergence.
    } finally {
      clearTimeout(deadline);
      this.active = undefined;
      this.arm();
    }
  }
}
