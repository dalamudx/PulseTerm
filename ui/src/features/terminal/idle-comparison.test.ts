import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { IdleComparison, type VisibleComparison } from "./idle-comparison";

const boundary = { columns: 32, rows: 8, revision: 7n, parsedBytes: 12n };
beforeEach(() => vi.useFakeTimers());
afterEach(() => vi.useRealTimers());
function fixture() {
  const compare = vi.fn().mockResolvedValue("equal");
  const mismatch = vi.fn();
  const idle = new IdleComparison({ compare, mismatch });
  return { idle, compare, mismatch };
}
it("checks only after idle and never polls an unchanged boundary", async () => {
  const { idle, compare } = fixture();
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(599);
  expect(compare).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(compare).toHaveBeenCalledTimes(1);
  await vi.advanceTimersByTimeAsync(10000);
  expect(compare).toHaveBeenCalledTimes(1);
  idle.dispose();
});
it("new output invalidates evidence even before its render completes", async () => {
  const { idle, compare } = fixture();
  idle.observe(boundary);
  idle.activity();
  await vi.advanceTimersByTimeAsync(1000);
  expect(compare).not.toHaveBeenCalled();
  idle.observe({ ...boundary, revision: 8n });
  await vi.advanceTimersByTimeAsync(600);
  expect(compare.mock.calls[0]?.[0].revision).toBe(8n);
  idle.dispose();
});
it("hidden views do not hash; showing a view checks once", async () => {
  const { idle, compare } = fixture();
  idle.setVisible(false);
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(2000);
  expect(compare).not.toHaveBeenCalled();
  idle.setVisible(true);
  await vi.advanceTimersByTimeAsync(600);
  expect(compare).toHaveBeenCalledTimes(1);
  idle.dispose();
});
it("stale and failures have finite retries and never request recovery", async () => {
  const { idle, compare, mismatch } = fixture();
  compare.mockRejectedValue(new Error("busy"));
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(10000);
  expect(compare).toHaveBeenCalledTimes(3);
  expect(mismatch).not.toHaveBeenCalled();
  compare.mockResolvedValue("stale");
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(10000);
  expect(compare).toHaveBeenCalledTimes(6);
  expect(mismatch).not.toHaveBeenCalled();
  idle.dispose();
});
it("mismatch emits evidence once, not an unbounded restore loop", async () => {
  const { idle, compare, mismatch } = fixture();
  compare.mockResolvedValue("mismatch");
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(10000);
  expect(mismatch).toHaveBeenCalledExactlyOnceWith(boundary);
  expect(compare).toHaveBeenCalledTimes(1);
  idle.dispose();
});
it("replacement/disposal prevents a late mismatch from reaching a new surface", async () => {
  const { idle, compare, mismatch } = fixture();
  let resolve!: (r: VisibleComparison) => void;
  compare.mockImplementation(
    () =>
      new Promise<VisibleComparison>((r) => {
        resolve = r;
      }),
  );
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(600);
  idle.dispose();
  resolve("mismatch");
  await vi.advanceTimersByTimeAsync(10000);
  expect(mismatch).not.toHaveBeenCalled();
  expect(compare).toHaveBeenCalledTimes(1);
});
it("deadline never permits parallel noncancellable IPC, and rejects late evidence", async () => {
  const { idle, compare, mismatch } = fixture();
  let resolve!: (r: VisibleComparison) => void;
  compare.mockImplementation(
    () =>
      new Promise<VisibleComparison>((r) => {
        resolve = r;
      }),
  );
  idle.observe(boundary);
  await vi.advanceTimersByTimeAsync(600);
  const signal = compare.mock.calls[0]?.[1] as AbortSignal;
  await vi.advanceTimersByTimeAsync(350);
  expect(signal.aborted).toBe(true);
  idle.observe({ ...boundary, revision: 8n });
  await vi.advanceTimersByTimeAsync(10000);
  expect(compare).toHaveBeenCalledTimes(1);
  resolve("mismatch");
  await vi.advanceTimersByTimeAsync(0);
  expect(mismatch).not.toHaveBeenCalled();
  compare.mockResolvedValue("equal");
  await vi.advanceTimersByTimeAsync(600);
  expect(compare).toHaveBeenCalledTimes(2);
  idle.dispose();
});
it("hide or activity rejects a pending comparison without overlap", async () => {
  for (const invalidate of [
    (i: IdleComparison) => i.activity(),
    (i: IdleComparison) => i.setVisible(false),
  ]) {
    const { idle, compare, mismatch } = fixture();
    let resolve!: (r: VisibleComparison) => void;
    compare.mockImplementation(
      () =>
        new Promise<VisibleComparison>((r) => {
          resolve = r;
        }),
    );
    idle.observe(boundary);
    await vi.advanceTimersByTimeAsync(600);
    invalidate(idle);
    resolve("mismatch");
    await vi.advanceTimersByTimeAsync(1000);
    expect(mismatch).not.toHaveBeenCalled();
    idle.dispose();
  }
});
