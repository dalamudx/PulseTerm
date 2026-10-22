import { afterEach, describe, expect, it, vi } from "vitest";
import { type FrameClock, renderFence } from "./render-fence";

it("reports existing animation opportunities without adding or skipping a fence", async () => {
  const c = clock();
  const opportunity = vi.fn();
  let done = false;
  const pending = renderFence(
    new AbortController().signal,
    () => true,
    c,
    () => true,
    opportunity,
  ).then(() => {
    done = true;
  });
  c.tick();
  await Promise.resolve();
  expect(opportunity).toHaveBeenCalledTimes(1);
  expect(done).toBe(false);
  c.tick();
  await pending;
  expect(opportunity).toHaveBeenCalledTimes(2);
  expect(done).toBe(true);
});
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
  it("requires two fresh animation opportunities after viewport invalidation", async () => {
    const c = clock();
    let stable = true;
    let completed = false;
    const pending = renderFence(
      new AbortController().signal,
      () => true,
      c,
      () => stable,
    ).then(() => {
      completed = true;
    });
    c.tick();
    stable = false;
    c.tick();
    stable = true;
    c.tick();
    await Promise.resolve();
    expect(completed).toBe(false);
    c.tick();
    await pending;
    expect(completed).toBe(true);
    expect(c.callbacks.size).toBe(0);
  });
  it("viewport invalidation never extends the original deadline", async () => {
    vi.useFakeTimers();
    const c = clock();
    let now = 0;
    const pending = renderFence(
      new AbortController().signal,
      () => true,
      { ...c, now: () => now },
      () => false,
    );
    const rejected = expect(pending).rejects.toThrow(
      "terminal.render_timeout.frames=0",
    );
    c.tick();
    now = 999;
    c.tick();
    now = 1000;
    await vi.advanceTimersByTimeAsync(1000);
    await rejected;
    expect(c.callbacks.size).toBe(0);
  });
  it("cancels a viewport redraw and rejects failures in its validity check", async () => {
    const c = clock();
    const abort = new AbortController();
    const pending = renderFence(
      abort.signal,
      () => true,
      c,
      () => false,
    );
    const rejected = expect(pending).rejects.toThrow("cancelled");
    c.tick();
    abort.abort();
    await rejected;
    expect(c.callbacks.size).toBe(0);
    const failed = renderFence(
      new AbortController().signal,
      () => true,
      c,
      () => {
        throw Error("view.retired");
      },
    );
    const failure = expect(failed).rejects.toThrow("view.retired");
    c.tick();
    await failure;
    expect(c.callbacks.size).toBe(0);
  });
  it("task completion still needs two frames and a later task", async () => {
    vi.useFakeTimers();
    const c = clock();
    let done = false;
    const pending = renderFence(
      new AbortController().signal,
      () => true,
      c,
      () => true,
      undefined,
      true,
    ).then(() => {
      done = true;
    });
    c.tick();
    c.tick();
    await Promise.resolve();
    expect(done).toBe(false);
    await vi.advanceTimersByTimeAsync(0);
    await pending;
    expect(done).toBe(true);
  });
  it.each(["abort", "hide", "detach", "deadline"])(
    "task completion rejects %s between the second frame and its task",
    async (mode) => {
      vi.useFakeTimers();
      const c = clock(),
        life = new AbortController();
      let visible = true,
        connected = true,
        now = 0;
      const pending = renderFence(
        life.signal,
        () => connected,
        { ...c, now: () => now, visible: () => visible },
        () => true,
        undefined,
        true,
      );
      const rejected = expect(pending).rejects.toThrow(
        mode === "abort"
          ? "cancelled"
          : mode === "deadline"
            ? "timeout"
            : "unavailable",
      );
      c.tick();
      c.tick();
      if (mode === "abort") life.abort();
      if (mode === "hide") visible = false;
      if (mode === "detach") connected = false;
      if (mode === "deadline") now = 1000;
      await vi.advanceTimersByTimeAsync(0);
      await rejected;
      expect(c.callbacks.size).toBe(0);
      expect(vi.getTimerCount()).toBe(0);
    },
  );
  it("viewport change before task completion requires two fresh frames", async () => {
    vi.useFakeTimers();
    const c = clock();
    let stable = true,
      done = false;
    const pending = renderFence(
      new AbortController().signal,
      () => true,
      c,
      () => stable,
      undefined,
      true,
    ).then(() => {
      done = true;
    });
    c.tick();
    c.tick();
    stable = false;
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    stable = true;
    c.tick();
    await vi.advanceTimersByTimeAsync(0);
    expect(done).toBe(false);
    c.tick();
    await vi.advanceTimersByTimeAsync(0);
    await pending;
    expect(done).toBe(true);
  });
});
