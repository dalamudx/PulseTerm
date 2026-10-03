import { afterEach, expect, it, vi } from "vitest";
import { InputQueue } from "./input-queue";

function pending() {
  let resolve!: () => void;
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
afterEach(() => vi.useRealTimers());
it("serializes UTF8 input and accounts for actual bytes incl active call", async () => {
  const gate = pending();
  const send = vi
    .fn()
    .mockImplementationOnce(() => gate.promise)
    .mockResolvedValue(undefined);
  const q = new InputQueue(send, new AbortController().signal);
  const first = q.enqueue("中文🌍"),
    second = q.enqueue("\r");
  expect(q.queuedBytes).toBe(11);
  expect(send).toHaveBeenCalledTimes(1);
  gate.resolve();
  await Promise.all([first, second]);
  expect(new TextDecoder().decode(send.mock.calls[0]?.[0])).toBe("中文🌍");
  expect(q.queuedBytes).toBe(0);
  q.cancel();
});
it("never retries uncertain admission or drains later jobs after a deadline", async () => {
  vi.useFakeTimers();
  const gate = pending(),
    send = vi.fn(() => gate.promise);
  const q = new InputQueue(send, new AbortController().signal);
  const first = expect(q.enqueue("a")).rejects.toThrow("uncertain"),
    second = expect(q.enqueue("b")).rejects.toThrow("uncertain");
  await vi.advanceTimersByTimeAsync(1000);
  await Promise.all([first, second]);
  gate.resolve();
  await Promise.resolve();
  expect(send).toHaveBeenCalledTimes(1);
  expect(q.queuedBytes).toBe(0);
  await expect(q.enqueue("c")).rejects.toThrow("closed");
});
it("revocation rejects active/queued jobs and ignores a late native success", async () => {
  const a = new AbortController(),
    gate = pending(),
    send = vi.fn(() => gate.promise);
  const q = new InputQueue(send, a.signal);
  const first = expect(q.enqueue("a")).rejects.toThrow("cancelled"),
    second = expect(q.enqueue("b")).rejects.toThrow("cancelled");
  a.abort();
  await Promise.all([first, second]);
  gate.resolve();
  await Promise.resolve();
  expect(send).toHaveBeenCalledTimes(1);
  expect(q.queuedBytes).toBe(0);
});
it("bounds UTF8 before dispatch without replacing malformed surrogate text", async () => {
  const send = vi.fn().mockResolvedValue(undefined),
    q = new InputQueue(send, new AbortController().signal);
  for (const text of ["", "x".repeat(1025), "\ud800", "\udc00"]) {
    await expect(q.enqueue(text)).rejects.toThrow("invalid");
  }
  await expect(q.enqueue("中".repeat(400))).rejects.toThrow("too_large");
  expect(send).not.toHaveBeenCalled();
  await q.enqueue("🌍");
  q.cancel();
});
it("fails closed on native errors and does not send later input", async () => {
  const send = vi.fn().mockRejectedValue(new Error("host detail")),
    q = new InputQueue(send, new AbortController().signal);
  const results = await Promise.allSettled([q.enqueue("a"), q.enqueue("b")]);
  expect(
    results.every(
      (r) => r.status === "rejected" && r.reason.message === "input.transport",
    ),
  ).toBe(true);
  expect(send).toHaveBeenCalledTimes(1);
  expect(q.queuedBytes).toBe(0);
});
it("bounds both job count and byte count including active IPC", async () => {
  for (const text of ["x", "x".repeat(1024)]) {
    const gate = pending(),
      send = vi.fn(() => gate.promise),
      q = new InputQueue(send, new AbortController().signal);
    const jobs = [];
    for (let i = 0; i < (text.length === 1 ? 32 : 8); i++)
      jobs.push(q.enqueue(text).catch((e: Error) => e.message));
    await expect(q.enqueue(text)).rejects.toThrow("overloaded");
    expect(
      (await Promise.all(jobs)).every((e) => e === "input.overloaded"),
    ).toBe(true);
    gate.resolve();
    expect(send).toHaveBeenCalledTimes(1);
    expect(q.queuedBytes).toBe(0);
  }
});
it("does not start any IPC when already revoked", async () => {
  const a = new AbortController();
  a.abort();
  const send = vi.fn();
  await expect(new InputQueue(send, a.signal).enqueue("a")).rejects.toThrow(
    "closed",
  );
  expect(send).not.toHaveBeenCalled();
});
