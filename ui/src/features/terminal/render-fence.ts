export interface FrameClock {
  request(callback: FrameRequestCallback): number;
  cancel(id: number): void;
  visible(): boolean;
}
const browser: FrameClock = {
  request: (callback) => requestAnimationFrame(callback),
  cancel: (id) => cancelAnimationFrame(id),
  visible: () => document.visibilityState === "visible",
};
/** Two animation opportunities after parser completion, NOT compositor ACK.
 * Hidden/disconnected/aborted surfaces fail closed; never skip a frame deadline. */
export function renderFence(
  signal: AbortSignal,
  connected: () => boolean,
  clock: FrameClock = browser,
): Promise<void> {
  return new Promise((resolve, reject) => {
    let frame: number | undefined;
    let timer: ReturnType<typeof setTimeout> | undefined;
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
      if (++count === 2) {
        finish();
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
    timer = setTimeout(
      () => finish(new Error("terminal.render_timeout")),
      1000,
    );
    frame = clock.request(step);
  });
}
