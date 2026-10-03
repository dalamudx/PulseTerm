import { afterEach, expect, it, vi } from "vitest";
import { FitScheduler, measuredGrid } from "./fit-scheduler";

afterEach(() => vi.useRealTimers());
function pending() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { resolve, promise };
}
it("uses measured cells and floors available dimensions with hard caps", () => {
  expect(measuredGrid(337, 159, 8, 16)).toEqual({ columns: 42, rows: 9 });
  expect(measuredGrid(9999, 9999, 8, 16)).toEqual({ columns: 160, rows: 60 });
});
it("does not invent geometry for hidden/invalid/small surfaces", () => {
  for (const n of [0, -1, NaN, Infinity])
    for (let i = 0; i < 4; i++) {
      const values = [320, 192, 8, 16];
      values[i] = n;
      expect(
        measuredGrid(...(values as [number, number, number, number])),
      ).toBeUndefined();
    }
  expect(measuredGrid(8, 16, 8, 16)).toBeUndefined();
});
it("coalesces storms and waits native acknowledgement before reattach", async () => {
  vi.useFakeTimers();
  const gate = pending(),
    send = vi.fn(() => gate.promise),
    replace = vi.fn(),
    fail = vi.fn();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    replace,
    fail,
    new AbortController().signal,
  );
  fit.observe({ columns: 50, rows: 12 });
  await vi.advanceTimersByTimeAsync(25);
  fit.observe({ columns: 60, rows: 12 });
  await vi.advanceTimersByTimeAsync(49);
  expect(send).not.toHaveBeenCalled();
  await vi.advanceTimersByTimeAsync(1);
  expect(send).toHaveBeenCalledWith({ columns: 60, rows: 12 });
  expect(replace).not.toHaveBeenCalled();
  fit.observe({ columns: 70, rows: 12 });
  gate.resolve();
  await Promise.resolve();
  expect(replace).toHaveBeenCalledWith({ columns: 60, rows: 12 });
  await vi.advanceTimersByTimeAsync(100);
  expect(send).toHaveBeenCalledTimes(1);
  expect(fail).not.toHaveBeenCalled();
});
it("rejects invalid directly supplied geometry before native dispatch", async () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    vi.fn(),
    vi.fn(),
    new AbortController().signal,
  );
  for (const size of [
    { columns: NaN, rows: 12 },
    { columns: 40.5, rows: 12 },
    { columns: 161, rows: 12 },
    { columns: 40, rows: 61 },
    { columns: 1, rows: 12 },
  ]) {
    fit.observe(size);
    await vi.advanceTimersByTimeAsync(100);
  }
  expect(send).not.toHaveBeenCalled();
  fit.dispose();
});

it("same-size and hidden measurements cancel pending work", async () => {
  vi.useFakeTimers();
  const send = vi.fn();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    vi.fn(),
    vi.fn(),
    new AbortController().signal,
  );
  fit.observe({ columns: 50, rows: 12 });
  fit.observe(undefined);
  await vi.advanceTimersByTimeAsync(100);
  fit.observe({ columns: 40, rows: 12 });
  await vi.advanceTimersByTimeAsync(100);
  expect(send).not.toHaveBeenCalled();
  fit.dispose();
});
it("revocation ignores late resize and does not authorize reattach", async () => {
  vi.useFakeTimers();
  const gate = pending(),
    send = vi.fn(() => gate.promise),
    replace = vi.fn(),
    fail = vi.fn(),
    a = new AbortController();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    replace,
    fail,
    a.signal,
  );
  fit.observe({ columns: 60, rows: 16 });
  await vi.advanceTimersByTimeAsync(50);
  a.abort();
  gate.resolve();
  await Promise.resolve();
  expect(replace).not.toHaveBeenCalled();
  expect(fail).not.toHaveBeenCalled();
});
it("uncertain deadline invalidates once without retry or late reattach", async () => {
  vi.useFakeTimers();
  const gate = pending(),
    send = vi.fn(() => gate.promise),
    replace = vi.fn(),
    fail = vi.fn();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    replace,
    fail,
    new AbortController().signal,
  );
  fit.observe({ columns: 60, rows: 16 });
  await vi.advanceTimersByTimeAsync(6050);
  expect(fail.mock.calls[0]?.[0].message).toBe("fit.uncertain");
  gate.resolve();
  await Promise.resolve();
  fit.observe({ columns: 80, rows: 20 });
  await vi.advanceTimersByTimeAsync(100);
  expect(send).toHaveBeenCalledTimes(1);
  expect(replace).not.toHaveBeenCalled();
});
it("native error never commits optimistic geometry", async () => {
  vi.useFakeTimers();
  const replace = vi.fn(),
    fail = vi.fn();
  const fit = new FitScheduler(
    { columns: 40, rows: 12 },
    vi.fn().mockRejectedValue(new Error("host detail")),
    replace,
    fail,
    new AbortController().signal,
  );
  fit.observe({ columns: 60, rows: 16 });
  await vi.advanceTimersByTimeAsync(50);
  expect(fail.mock.calls[0]?.[0].message).toBe("fit.native_failed");
  expect(replace).not.toHaveBeenCalled();
});
it("already-revoked scheduler never starts native work", async () => {
  vi.useFakeTimers();
  const a = new AbortController();
  a.abort();
  const send = vi.fn();
  new FitScheduler(
    { columns: 40, rows: 12 },
    send,
    vi.fn(),
    vi.fn(),
    a.signal,
  ).observe({ columns: 60, rows: 16 });
  await vi.advanceTimersByTimeAsync(50);
  expect(send).not.toHaveBeenCalled();
});
