import { afterEach, describe, expect, it, vi } from "vitest";
import { type FrameClock, renderFence } from "./render-fence";

function clock() {
  let id = 0;
  const callbacks = new Map<number, FrameRequestCallback>();
  return {
    request: (fn: FrameRequestCallback) => {
      callbacks.set(++id, fn);
      return id;
    },
    cancel: vi.fn((i: number) => callbacks.delete(i)),
    visible: () => true,
    tick() {
      const all = [...callbacks.values()];
      callbacks.clear();
      for (const fn of all) fn(0);
    },
    callbacks,
  } satisfies FrameClock & {
    tick(): void;
    callbacks: Map<number, FrameRequestCallback>;
  };
}
afterEach(() => vi.useRealTimers());
describe("DOM frame fence", () => {
  it("does not release credit after only one animation opportunity", async () => {
    const c = clock();
    let done = false;
    const p = renderFence(new AbortController().signal, () => true, c).then(
      () => {
        done = true;
      },
    );
    c.tick();
    await Promise.resolve();
    expect(done).toBe(false);
    c.tick();
    await p;
    expect(done).toBe(true);
    expect(c.callbacks.size).toBe(0);
  });
  it("cancels scheduled work on abort", async () => {
    const c = clock(),
      a = new AbortController();
    const p = renderFence(a.signal, () => true, c);
    const rejected = expect(p).rejects.toThrow("cancelled");
    a.abort();
    await rejected;
    expect(c.callbacks.size).toBe(0);
  });
  it("rejects an already cancelled lifetime", async () => {
    const a = new AbortController();
    a.abort();
    const c = clock();
    await expect(renderFence(a.signal, () => true, c)).rejects.toThrow(
      "cancelled",
    );
    expect(c.callbacks.size).toBe(0);
  });
  it("rejects hidden and disconnected surfaces rather than bypass the wait", async () => {
    await expect(
      renderFence(new AbortController().signal, () => true, {
        ...clock(),
        visible: () => false,
      }),
    ).rejects.toThrow("unavailable");
    await expect(
      renderFence(new AbortController().signal, () => false, clock()),
    ).rejects.toThrow("unavailable");
  });
  it("rejects detach between frames", async () => {
    const c = clock();
    let connected = true;
    const p = renderFence(new AbortController().signal, () => connected, c);
    const rejection = expect(p).rejects.toThrow("unavailable");
    c.tick();
    connected = false;
    c.tick();
    await rejection;
  });
  it("reports a delayed timer without pretending RAF ran", async () => {
    vi.useFakeTimers();
    const c = clock();
    let time = 0;
    const p = renderFence(new AbortController().signal, () => true, {
      ...c,
      now: () => time,
      focused: () => false,
    });
    const rejection = expect(p).rejects.toThrow(
      "terminal.render_timeout.frames=0.visible=true.connected=true.elapsedMs=2400.focused=false",
    );
    time = 2400;
    await vi.advanceTimersByTimeAsync(1000);
    await rejection;
    expect(c.callbacks.size).toBe(0);
  });
  it("focused metadata cannot release credit with only one frame", async () => {
    vi.useFakeTimers();
    const c = clock();
    let time = 100;
    const p = renderFence(new AbortController().signal, () => true, {
      ...c,
      now: () => time,
      focused: () => true,
    });
    const rejection = expect(p).rejects.toThrow(
      "terminal.render_timeout.frames=1.visible=true.connected=true.elapsedMs=1100.focused=true",
    );
    c.tick();
    time = 1200;
    await vi.advanceTimersByTimeAsync(1000);
    await rejection;
    expect(c.callbacks.size).toBe(0);
  });
  it.each([0, 1])(
    "rejects overdue RAF before timer dispatch after %i accepted frames",
    async (earlyFrames) => {
      vi.useFakeTimers();
      const c = clock();
      let time = 0;
      const p = renderFence(new AbortController().signal, () => true, {
        ...c,
        now: () => time,
        focused: () => true,
      });
      const rejection = expect(p).rejects.toThrow(
        `terminal.render_timeout.frames=${earlyFrames}.visible=true.connected=true.elapsedMs=1000.focused=true`,
      );
      if (earlyFrames === 1) {
        time = 10;
        c.tick();
      }
      time = 1000;
      c.tick(); // Deliberately do not dispatch the 1000ms timer before RAF.
      await rejection;
      expect(c.callbacks.size).toBe(0);
      await vi.advanceTimersByTimeAsync(1000);
      expect(c.callbacks.size).toBe(0);
    },
  );
  it("has a bounded deadline even when RAF never fires", async () => {
    vi.useFakeTimers();
    const c = clock();
    const p = renderFence(new AbortController().signal, () => true, c);
    const rejection = expect(p).rejects.toThrow("timeout");
    await vi.advanceTimersByTimeAsync(1000);
    await rejection;
    expect(c.callbacks.size).toBe(0);
  });
});
