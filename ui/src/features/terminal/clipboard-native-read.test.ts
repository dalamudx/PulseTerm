import { expect, it, vi } from "vitest";
import { readPreparedClipboard } from "./clipboard-read";

const grant = "11111111-1111-4111-8111-111111111111";
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function fixture() {
  const abort = new AbortController();
  const ports = {
    prepare: vi.fn(async (): Promise<unknown> => grant),
    read: vi.fn(async (_grant: string): Promise<unknown> => "fixture"),
    cancel: vi.fn(async (_grant: string): Promise<unknown> => undefined),
  };
  return { abort, ports };
}
it("returns only the original prepared read and cancels that grant once", async () => {
  const f = fixture();
  expect(await readPreparedClipboard(f.abort.signal, () => true, f.ports)).toBe(
    "fixture",
  );
  expect(f.ports.read).toHaveBeenCalledExactlyOnceWith(grant);
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
});
it("late preparation after cancellation never reads and retires only its original grant", async () => {
  const f = fixture();
  const d = deferred<unknown>();
  f.ports.prepare.mockImplementation(() => d.promise);
  const pending = readPreparedClipboard(f.abort.signal, () => true, f.ports);
  const rejected = expect(pending).rejects.toThrow("paste.cancelled");
  f.abort.abort();
  d.resolve(grant);
  await rejected;
  expect(f.ports.read).not.toHaveBeenCalled();
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
});
it("cancellation waits for actual native read completion while retiring its token immediately", async () => {
  const f = fixture();
  const d = deferred<unknown>();
  f.ports.read.mockImplementation(() => d.promise);
  let finished = false;
  const pending = readPreparedClipboard(
    f.abort.signal,
    () => true,
    f.ports,
  ).finally(() => {
    finished = true;
  });
  const rejected = expect(pending).rejects.toThrow("paste.cancelled");
  await Promise.resolve();
  f.abort.abort();
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
  await Promise.resolve();
  expect(finished).toBe(false);
  d.resolve("retired");
  await rejected;
  expect(f.ports.cancel).toHaveBeenCalledOnce();
});
it("old scope or prior cancellation does no native operation", async () => {
  for (const aborted of [true, false]) {
    const f = fixture();
    if (aborted) f.abort.abort();
    await expect(
      readPreparedClipboard(f.abort.signal, () => aborted, f.ports),
    ).rejects.toThrow("paste.cancelled");
    expect(f.ports.prepare).not.toHaveBeenCalled();
    expect(f.ports.read).not.toHaveBeenCalled();
    expect(f.ports.cancel).not.toHaveBeenCalled();
  }
});
it.each([null, {}, "bad", 42])(
  "invalid native grant metadata cannot read or cancel",
  async (value) => {
    const f = fixture();
    f.ports.prepare.mockResolvedValue(value);
    await expect(
      readPreparedClipboard(f.abort.signal, () => true, f.ports),
    ).rejects.toThrow("paste.clipboard_unavailable");
    expect(f.ports.read).not.toHaveBeenCalled();
    expect(f.ports.cancel).not.toHaveBeenCalled();
  },
);
it("late text after view retirement is discarded", async () => {
  const f = fixture();
  const d = deferred<unknown>();
  f.ports.read.mockImplementation(() => d.promise);
  let current = true;
  const pending = readPreparedClipboard(f.abort.signal, () => current, f.ports);
  const rejected = expect(pending).rejects.toThrow("paste.cancelled");
  await Promise.resolve();
  current = false;
  d.resolve("retired");
  await rejected;
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
});
it("native denial is not retried and retires only that read", async () => {
  const f = fixture();
  f.ports.read.mockRejectedValue(Error("denied"));
  await expect(
    readPreparedClipboard(f.abort.signal, () => true, f.ports),
  ).rejects.toThrow("denied");
  expect(f.ports.read).toHaveBeenCalledOnce();
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
});
it("malformed text responses are rejected without a second read", async () => {
  const f = fixture();
  f.ports.read.mockResolvedValue({ text: "fixture" });
  await expect(
    readPreparedClipboard(f.abort.signal, () => true, f.ports),
  ).rejects.toThrow("paste.clipboard_unavailable");
  expect(f.ports.read).toHaveBeenCalledOnce();
  expect(f.ports.cancel).toHaveBeenCalledExactlyOnceWith(grant);
});
