import { expect, it, vi } from "vitest";
import { TransferViewProbe } from "./transfer-view-probe";

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((r) => {
    resolve = r;
  });
  return { resolve, promise };
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
  return [
    wire(version, 1, 0, 0n, meta),
    wire(version, 3, 1, 0n, restore),
    wire(version, 4, 2, 0n, new Uint8Array()),
  ];
}
function wire(
  version: 1 | 2,
  part: number,
  sequence: number,
  offset: bigint,
  body: Uint8Array,
) {
  const envelope = version === 1 ? 20 : 28;
  const bytes = new Uint8Array(6 + envelope + body.length);
  const v = new DataView(bytes.buffer);
  bytes[0] = 8;
  v.setUint32(2, sequence, true);
  bytes[6] = version;
  bytes[7] = part;
  v.setBigUint64(10, 17n, true);
  v.setBigUint64(18, offset, true);
  if (version === 2) v.setBigUint64(26, part <= 4 ? 19n : offset, true);
  bytes.set(body, 6 + envelope);
  return bytes.buffer;
}
function revision(value: bigint) {
  const b = new Uint8Array(8);
  new DataView(b.buffer).setBigUint64(0, value, true);
  return b;
}
for (const version of [1, 2] as const) {
  it(`v${version} retained readonly view consumes live Data and pure Marker after callback ACK without finishing half UTF8`, async () => {
    const hold = deferred(),
      arrived = deferred();
    const markers: string[] = [];
    const ready = vi.fn(async () => {});
    const failed = vi.fn();
    const confirm = vi.fn(async () => {});
    const marker = vi.fn(async (_generation, boundary) => {
      markers.push(boundary.parsedBytes.toString());
    });
    let text = "";
    const life = new AbortController();
    const view = new TransferViewProbe({
      version,
      signal: life.signal,
      ack: async (_g, s) => {
        if (s === 4) {
          arrived.resolve();
          await hold.promise;
        }
      },
      confirm,
      digest: (terminal) => {
        text = terminal.buffer.active.getLine(0)?.translateToString(true) ?? "";
        return "a".repeat(16);
      },
      failed,
      live: { ready, marker },
    });
    for (const frame of frames(version)) view.receive(frame);
    await view.finish();
    expect(ready).toHaveBeenCalledOnce();
    expect(confirm).toHaveBeenCalledOnce();
    view.receive(wire(version, 5, 3, 19n, new Uint8Array([0x20, 0xf0, 0x9f])));
    view.receive(wire(version, 8, 4, 22n, revision(4n)));
    await arrived.promise;
    expect(marker).not.toHaveBeenCalled();
    hold.resolve();
    await view.finish();
    expect(markers).toEqual(["22"]);
    expect(text).toBe("hi中 ");
    view.receive(wire(version, 5, 5, 22n, new Uint8Array([0x8c, 0x8d])));
    view.receive(wire(version, 8, 6, 24n, revision(5n)));
    await view.finish();
    expect(text).toBe("hi中 🌍");
    expect(markers).toEqual(["22", "24"]);
    expect(confirm).toHaveBeenCalledOnce();
    view.receive(wire(version, 5, 7, 24n, new TextEncoder().encode("\x1b[6n")));
    view.receive(wire(version, 8, 8, 28n, revision(6n)));
    await view.finish();
    expect(text).toBe("hi中 🌍");
    expect(markers).toEqual(["22", "24", "28"]);
    expect(failed).not.toHaveBeenCalled();
    life.abort();
    view.receive(wire(version, 5, 9, 28n, new Uint8Array([65])));
    await expect(view.finish()).rejects.toThrow("transfer.cancelled");
    expect(marker).toHaveBeenCalledTimes(3);
    view.dispose();
  });
  it(`v${version} live marker confirmation failure aborts the retained view instead of claiming recovery`, async () => {
    const failed = vi.fn();
    const marker = vi.fn(async () => {
      throw new Error("marker.mismatch");
    });
    const view = new TransferViewProbe({
      version,
      signal: new AbortController().signal,
      ack: async () => {},
      confirm: async () => {},
      digest: () => "a".repeat(16),
      failed,
      live: { ready: async () => {}, marker },
    });
    for (const frame of frames(version)) view.receive(frame);
    await view.finish();
    view.receive(wire(version, 8, 3, 19n, revision(4n)));
    await expect(view.finish()).rejects.toThrow("marker.mismatch");
    expect(failed).toHaveBeenCalledOnce();
    view.receive(wire(version, 5, 4, 19n, new Uint8Array([65])));
    await expect(view.finish()).rejects.toThrow("marker.mismatch");
    expect(marker).toHaveBeenCalledOnce();
    view.dispose();
  });
  it(`v${version} snapshot still refuses continuation even after commit`, async () => {
    const failed = vi.fn();
    const view = new TransferViewProbe({
      version,
      signal: new AbortController().signal,
      ack: async () => {},
      confirm: async () => {},
      digest: () => "a".repeat(16),
      failed,
    });
    for (const frame of frames(version)) view.receive(frame);
    await view.finish();
    view.receive(wire(version, 5, 3, 19n, new Uint8Array([65])));
    await expect(view.finish()).rejects.toThrow("transfer.unexpected_frame");
    expect(failed).toHaveBeenCalledOnce();
    view.dispose();
  });
}
