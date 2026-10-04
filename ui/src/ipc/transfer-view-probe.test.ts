import { expect, it, vi } from "vitest";
import { TransferViewProbe } from "./transfer-view-probe";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function frames(version: 1 | 2) {
  const restore = new TextEncoder().encode("\x1b[Hhi中");
  const meta = new Uint8Array(version === 1 ? 28 : 36);
  const m = new DataView(meta.buffer);
  m.setUint16(0, 8, true);
  m.setUint16(2, 2, true);
  m.setBigUint64(4, 3n, true);
  m.setBigUint64(12, 19n, true);
  if (version === 2) m.setBigUint64(20, 19n, true);
  m.setUint32(version === 1 ? 24 : 32, restore.length, true);
  const wire = (part: number, seq: number, body: Uint8Array) => {
    const envelope = version === 1 ? 20 : 28;
    const bytes = new Uint8Array(6 + envelope + body.length);
    const v = new DataView(bytes.buffer);
    bytes[0] = 8;
    v.setUint32(2, seq, true);
    bytes[6] = version;
    bytes[7] = part;
    v.setBigUint64(10, 17n, true);
    if (version === 2) v.setBigUint64(26, 19n, true);
    bytes.set(body, 6 + envelope);
    return bytes.buffer;
  };
  return [wire(1, 0, meta), wire(3, 1, restore), wire(4, 2, new Uint8Array())];
}
for (const version of [1, 2] as const) {
  it(`v${version} independent transfer queue renders Chinese before commit ACK and confirms exact boundary once`, async () => {
    const hold = deferred(),
      arrived = deferred();
    const ack: number[] = [];
    const confirm = vi.fn(async (g, b, d) => {
      expect(g).toBe("17");
      expect(b.parsedBytes).toBe(19n);
      expect(b.revision).toBe(3n);
      expect(d).toBe("a".repeat(16));
    });
    const failed = vi.fn();
    const view = new TransferViewProbe({
      version,
      signal: new AbortController().signal,
      ack: async (_g, s) => {
        ack.push(s);
        if (s === 2) {
          arrived.resolve();
          await hold.promise;
        }
      },
      confirm,
      digest: (terminal) => {
        expect(terminal.buffer.active.getLine(0)?.translateToString(true)).toBe(
          "hi中",
        );
        return "a".repeat(16);
      },
      failed,
    });
    for (const wire of frames(version)) view.receive(wire);
    await arrived.promise;
    expect(ack).toEqual([0, 1, 2]);
    expect(confirm).not.toHaveBeenCalled();
    hold.resolve();
    await view.finish();
    expect(confirm).toHaveBeenCalledTimes(1);
    expect(failed).not.toHaveBeenCalled();
    view.dispose();
  });
}
it("parent cancellation during ACK rejects recovery and never accepts late confirmation", async () => {
  const parent = new AbortController(),
    hold = deferred(),
    arrived = deferred();
  const confirm = vi.fn(async () => {});
  const failed = vi.fn();
  const view = new TransferViewProbe({
    version: 2,
    signal: parent.signal,
    ack: async (_g, s) => {
      if (s === 2) {
        arrived.resolve();
        await hold.promise;
      }
    },
    confirm,
    digest: () => "a".repeat(16),
    failed,
  });
  for (const wire of frames(2)) view.receive(wire);
  await arrived.promise;
  parent.abort();
  hold.resolve();
  await expect(view.finish()).rejects.toThrow();
  expect(confirm).not.toHaveBeenCalled();
  view.dispose();
});
it("wrong codec, no Begin and frames after successful Commit fail closed", async () => {
  for (const [version, wire] of [
    [1, frames(2)[0]],
    [2, frames(2)[1]],
  ] as const) {
    if (!wire) throw new Error("test fixture");
    const failed = vi.fn();
    const view = new TransferViewProbe({
      version,
      signal: new AbortController().signal,
      ack: async () => {},
      confirm: async () => {},
      digest: () => "a".repeat(16),
      failed,
    });
    view.receive(wire);
    await expect(view.finish()).rejects.toThrow();
    expect(failed).toHaveBeenCalledTimes(1);
  }
  const failed = vi.fn(),
    confirm = vi.fn(async () => {});
  const view = new TransferViewProbe({
    version: 1,
    signal: new AbortController().signal,
    ack: async () => {},
    confirm,
    digest: () => "a".repeat(16),
    failed,
  });
  for (const wire of frames(1)) view.receive(wire);
  await view.finish();
  const late = frames(1)[2];
  if (!late) throw new Error("test fixture");
  view.receive(late);
  await expect(view.finish()).rejects.toThrow();
  expect(confirm).toHaveBeenCalledTimes(1);
  expect(failed).toHaveBeenCalledTimes(1);
});
it("confirmation failure cannot be treated as a checked migration", async () => {
  const failed = vi.fn();
  const view = new TransferViewProbe({
    version: 2,
    signal: new AbortController().signal,
    ack: async () => {},
    confirm: async () => {
      throw new Error("native mismatch");
    },
    digest: () => "a".repeat(16),
    failed,
  });
  for (const wire of frames(2)) view.receive(wire);
  await expect(view.finish()).rejects.toThrow("native mismatch");
  expect(failed).toHaveBeenCalledTimes(1);
  view.dispose();
});
