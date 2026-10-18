export interface FrameClock {
  request(callback: FrameRequestCallback): number;
  cancel(id: number): void;
  visible(): boolean;
  /** Optional metadata only; never substitutes for an animation opportunity. */
  now?(): number;
  focused?(): boolean;
}
const browser: FrameClock = {
  request: (callback) => requestAnimationFrame(callback),
  cancel: (id) => cancelAnimationFrame(id),
  visible: () => document.visibilityState === "visible",
  now: () => performance.now(),
  focused: () => document.hasFocus(),
};
/** Two animation opportunities after parser completion, NOT compositor ACK.
 * Hidden/disconnected/aborted surfaces fail closed; never skip a frame deadline. */
export function renderFence(
  signal: AbortSignal,
  connected: () => boolean,
  clock: FrameClock = browser,
  current: () => boolean = () => true,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let frame: number | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
    const started = clock.now?.() ?? performance.now();
    let count = 0;
    let done = false;
    const finish = (error?: Error) => {
      if (done) return;
      done = true;
      if (frame !== undefined) clock.cancel(frame);
      clearTimeout(timer);
      signal.removeEventListener("abort", abort);
      if (error) reject(error);
      else resolve();
    };
    const abort = () => finish(new Error("terminal.render_cancelled"));
    const elapsed = () => (clock.now?.() ?? performance.now()) - started;
    const timeout = () =>
      finish(
        new Error(
          `terminal.render_timeout.frames=${count}.visible=${clock.visible()}.connected=${connected()}.elapsedMs=${Math.max(0, Math.round(elapsed()))}.focused=${clock.focused?.() ?? "unknown"}`,
        ),
      );
    const step = () => {
      frame = undefined;
      if (signal.aborted) {
        abort();
        return;
      }
      if (!clock.visible() || !connected()) {
        finish(new Error("terminal.render_unavailable"));
        return;
      }
      // RAF may be dispatched before an overdue timer; elapsed time remains authoritative.
      if (elapsed() >= 1000) {
        timeout();
        return;
      }
      try {
        if (!current()) {
          // Repaint a moved viewport without extending the original deadline.
          count = 0;
        } else if (++count === 2) {
          finish();
          return;
        }
      } catch (error) {
        finish(
          error instanceof Error
            ? error
            : new Error("terminal.render_unavailable"),
        );
        return;
      }
      frame = clock.request(step);
    };
    signal.addEventListener("abort", abort, { once: true });
    if (signal.aborted) {
      abort();
      return;
    }
    if (!clock.visible() || !connected()) {
      finish(new Error("terminal.render_unavailable"));
      return;
    }
    timer = setTimeout(timeout, 1000);
    frame = clock.request(step);
  });
}
