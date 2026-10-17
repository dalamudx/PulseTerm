import { afterEach, expect, it, vi } from "vitest";
import type { Geometry } from "./fit-scheduler";
import { WorkspaceFit } from "./workspace-fit";

afterEach(() => vi.useRealTimers());
function fixture() {
  let target: Geometry | undefined = { columns: 80, rows: 24 };
  let finish!: () => void;
  const promise = new Promise<void>((r) => {
    finish = r;
  });
  const a = new AbortController();
  const view = {
    geometry: () => ({ columns: 80, rows: 24 }),
    measuredDimensions: () => target,
    pauseForResize: vi.fn(),
    resize: vi.fn(() => promise),
    watchGeometry: vi.fn((_listener: () => void) => vi.fn()),
  };
  const cb = {
    paused: vi.fn(),
    replaced: vi.fn(),
    failed: vi.fn(),
    settled: vi.fn(),
  };
  const fit = new WorkspaceFit(view, cb, a.signal);
  return {
    view,
    cb,
    fit,
    a,
    finish,
    set: (v: Geometry | undefined) => {
      target = v;
      fit.observe();
    },
  };
}
it("does not resize or pause a fitting/hidden host; settle is once", () => {
  const f = fixture();
  f.fit.observe();
  f.fit.observe();
  f.set(undefined);
  expect(f.cb.settled).toHaveBeenCalledOnce();
  expect(f.view.pauseForResize).not.toHaveBeenCalled();
  f.fit.dispose();
});
it("pauses immediately and coalesces latest targets before native dispatch", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  expect(f.view.pauseForResize).toHaveBeenCalledOnce();
  f.set({ columns: 100, rows: 30 });
  await vi.advanceTimersByTimeAsync(50);
  expect(f.view.resize).toHaveBeenCalledExactlyOnceWith({
    columns: 100,
    rows: 30,
  });
  f.set({ columns: 120, rows: 40 });
  expect(f.cb.replaced).not.toHaveBeenCalled();
  f.finish();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.cb.replaced).toHaveBeenCalledOnce();
});
it("return to original grid before dispatch rebuilds rather than reviving cancelled input", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  f.set({ columns: 80, rows: 24 });
  await vi.advanceTimersByTimeAsync(50);
  expect(f.view.resize).not.toHaveBeenCalled();
  expect(f.cb.replaced).toHaveBeenCalledOnce();
});
it("native uncertainty never retries or activates a late replacement", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  await vi.advanceTimersByTimeAsync(6050);
  expect(f.cb.failed).toHaveBeenCalledOnce();
  f.finish();
  await Promise.resolve();
  expect(f.cb.replaced).not.toHaveBeenCalled();
  expect(f.view.resize).toHaveBeenCalledOnce();
});
it("hidden measurements after pause do not dispatch zero grid and restore recaptures", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  f.set(undefined);
  await vi.advanceTimersByTimeAsync(100);
  expect(f.view.resize).not.toHaveBeenCalled();
  expect(f.cb.failed).not.toHaveBeenCalled();
  f.set({ columns: 80, rows: 24 });
  expect(f.cb.replaced).toHaveBeenCalledOnce();
  expect(f.view.pauseForResize).toHaveBeenCalledOnce();
});
it("return to original grid after native dispatch waits for ACK then rebuilds, no second resize", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  await vi.advanceTimersByTimeAsync(50);
  f.set({ columns: 80, rows: 24 });
  expect(f.cb.replaced).not.toHaveBeenCalled();
  f.finish();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.cb.replaced).toHaveBeenCalledOnce();
  expect(f.view.resize).toHaveBeenCalledOnce();
});
it("proactive geometry hook pauses before ResizeObserver and is removed on retirement", () => {
  const f = fixture();
  const hook = f.view.watchGeometry.mock.calls[0]?.[0];
  if (!hook) throw new Error("missing geometry observer");
  f.set({ columns: 90, rows: 28 });
  hook();
  expect(f.view.pauseForResize).toHaveBeenCalledOnce();
  f.a.abort();
  expect(f.view.watchGeometry.mock.results[0]?.value).toHaveBeenCalledOnce();
  hook();
  expect(f.cb.replaced).not.toHaveBeenCalled();
});
it("disposed or retired page ignores late native result", async () => {
  vi.useFakeTimers();
  const f = fixture();
  f.set({ columns: 90, rows: 28 });
  await vi.advanceTimersByTimeAsync(50);
  f.a.abort();
  f.finish();
  await Promise.resolve();
  expect(f.cb.replaced).not.toHaveBeenCalled();
  expect(f.cb.failed).not.toHaveBeenCalled();
});
