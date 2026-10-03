export interface Geometry {
  columns: number;
  rows: number;
}
/** Pure measured grid calculation. Never clamp a hidden/zero surface into a PTY. */
export function measuredGrid(
  width: number,
  height: number,
  cellWidth: number,
  cellHeight: number,
): Geometry | undefined {
  if (
    ![width, height, cellWidth, cellHeight].every(
      (n) => Number.isFinite(n) && n > 0,
    )
  )
    return undefined;
  const columns = Math.floor(width / cellWidth),
    rows = Math.floor(height / cellHeight);
  if (columns < 2 || rows < 2) return undefined;
  return { columns: Math.min(columns, 160), rows: Math.min(rows, 60) };
}
/** One viewer generation. Observer storms coalesce; native errors/timeouts have
 * uncertain side effects and MUST invalidate instead of retrying or optimistic fit.
 * A changed-size success invokes reattach; further sizes belong to the new generation. */
export class FitScheduler {
  private timer: ReturnType<typeof setTimeout> | undefined;
  private deadline: ReturnType<typeof setTimeout> | undefined;
  private desired: Geometry | undefined;
  private active = false;
  private closed = false;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly current: Geometry,
    private readonly resize: (size: Geometry) => Promise<void>,
    private readonly replaced: (size: Geometry) => void,
    private readonly failed: (error: Error) => void,
    private readonly signal: AbortSignal,
  ) {
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  observe(size: Geometry | undefined): void {
    if (this.closed) return;
    if (
      size &&
      (!Number.isInteger(size.columns) ||
        !Number.isInteger(size.rows) ||
        size.columns < 2 ||
        size.columns > 160 ||
        size.rows < 2 ||
        size.rows > 60)
    )
      size = undefined;
    this.desired = size ? { ...size } : undefined;
    clearTimeout(this.timer);
    if (
      this.active ||
      !size ||
      (size.columns === this.current.columns && size.rows === this.current.rows)
    )
      return;
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.run();
    }, 50);
  }
  dispose() {
    if (this.closed) return;
    this.closed = true;
    clearTimeout(this.timer);
    clearTimeout(this.deadline);
    this.signal.removeEventListener("abort", this.abort);
  }
  private error(message: string) {
    if (this.closed) return;
    this.dispose();
    this.failed(new Error(message));
  }
  private async run() {
    const size = this.desired;
    if (this.closed || this.active || !size) return;
    this.active = true;
    this.deadline = setTimeout(() => this.error("fit.uncertain"), 6000);
    try {
      await this.resize(size);
      if (this.closed || this.signal.aborted) return;
      this.dispose();
      this.replaced(size);
    } catch {
      this.error("fit.native_failed");
    }
  }
}
