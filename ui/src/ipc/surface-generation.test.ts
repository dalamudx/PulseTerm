import { expect, it, vi } from "vitest";
import { attachmentEnvelope } from "./attachment";
import { forCurrentSurface, SurfaceGeneration } from "./surface-generation";

it("requires Begin before output and replacement permission before a future generation", () => {
  const g = new SurfaceGeneration();
  expect(() => g.accept(1n, false, 1)).toThrow("no_begin");
  expect(g.accept(1n, true, 0)).toBe("initial");
  expect(g.accept(1n, false, 1)).toBe("same");
  expect(() => g.accept(1n, true, 0)).toThrow("duplicate");
  expect(() => g.accept(2n, true, 0)).toThrow("unrequested");
});
it("ignores old frames and old Begin without changing the current generation", () => {
  const g = new SurfaceGeneration();
  g.accept(1n, true, 0);
  g.expectReplacement(1n);
  g.accept(2n, true, 0);
  expect(g.accept(1n, false, 100)).toBe("stale");
  expect(g.accept(1n, true, 0)).toBe("stale");
  expect(g.accept(2n, false, 1)).toBe("same");
  expect(() => g.expectReplacement(1n)).toThrow("stale");
});
it("permits a series of requested resizes and resume but never parallel permissions", () => {
  const g = new SurfaceGeneration();
  g.accept(10n, true, 0);
  for (let i = 10n; i < 14n; i++) {
    g.expectReplacement(i);
    expect(() => g.expectReplacement(i)).toThrow("busy");
    expect(() => g.accept(i + 1n, false, 0)).toThrow("unrequested");
    expect(g.accept(i + 1n, true, 0)).toBe("replacement");
  }
  expect(g.accept(14n, false, 1)).toBe("same");
});
it("retains unsigned64 precision and rejects invalid identities", () => {
  const g = new SurfaceGeneration();
  g.accept(9007199254740993n, true, 0);
  g.expectReplacement(9007199254740993n);
  expect(g.accept(9007199254740994n, true, 0)).toBe("replacement");
  for (const n of [0n, -1n, 1n << 64n])
    expect(() => g.accept(n, true, 0)).toThrow("generation");
});
it("suppresses both success and rejection from cancelled old consumers", async () => {
  let current = true;
  let resolve!: (value: number) => void;
  const result = new Promise<number>((done) => {
      resolve = done;
    }),
    success = vi.fn().mockResolvedValue(undefined),
    failure = vi.fn();
  const p = forCurrentSurface(result, () => current, success, failure);
  current = false;
  resolve(1);
  await p;
  expect(success).not.toHaveBeenCalled();
  await forCurrentSurface(
    Promise.reject(new Error("cancelled")),
    () => false,
    success,
    failure,
  );
  expect(failure).not.toHaveBeenCalled();
});
it("does not detach the new surface after an old callback fails late", async () => {
  let current = true;
  let reject!: (e: Error) => void;
  const job = new Promise<void>((_, fail) => {
      reject = fail;
    }),
    error = vi.fn();
  const result = forCurrentSurface(
    Promise.resolve(1),
    () => current,
    () => job,
    error,
  );
  await Promise.resolve();
  current = false;
  reject(new Error("old"));
  await result;
  expect(error).not.toHaveBeenCalled();
});
it("supervises current failures and processes current callbacks", async () => {
  const success = vi.fn().mockResolvedValue(undefined),
    error = vi.fn();
  await forCurrentSurface(Promise.resolve(1), () => true, success, error);
  expect(success).toHaveBeenCalledWith(1);
  await forCurrentSurface(
    Promise.reject(new Error("current")),
    () => true,
    success,
    error,
  );
  expect(error).toHaveBeenCalledTimes(1);
});
it("uses bounded shared envelope validation before routing", () => {
  const payload = new Uint8Array(20),
    v = new DataView(payload.buffer);
  v.setUint8(0, 1);
  v.setUint8(1, 5);
  v.setBigUint64(4, 9007199254740993n, true);
  expect(attachmentEnvelope(payload).generation).toBe(9007199254740993n);
  v.setUint16(2, 1, true);
  expect(() => attachmentEnvelope(payload)).toThrow("flags");
  expect(() => attachmentEnvelope(new Uint8Array(19))).toThrow("length");
});
