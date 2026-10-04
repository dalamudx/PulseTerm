import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import { AttachmentReceiver } from "./attachment";
import {
  AttachmentV2Consumer,
  AttachmentV2Receiver,
  type V2Sink,
} from "./attachment-v2";

const generation = (1n << 60n) + 7n,
  base = 1n << 60n;
const wires = readFileSync(
  new URL("../../../tests/fixtures/frames/attachment-v2.hex", import.meta.url),
  "utf8",
)
  .trim()
  .split("\n")
  .map((line) =>
    Uint8Array.from(line.split(/\s+/), (n) => Number.parseInt(n, 16)),
  );
function frame(i: number) {
  const w = wires[i];
  if (!w) throw new Error("fixture");
  return w.slice();
}
function feed(r: AttachmentV2Receiver, n = 3) {
  for (const w of wires.slice(0, n)) r.accept(w);
}
const sink = (): V2Sink => ({
  reset: vi.fn(async () => {}),
  write: vi.fn(async () => {}),
  marker: vi.fn(async () => {}),
  repair: vi.fn(async () => {}),
  finish: vi.fn(async () => {}),
});
it("preserves independent bigint source and display cursors including pending input", () => {
  const r = new AttachmentV2Receiver(generation);
  r.accept(frame(0));
  r.accept(frame(1));
  expect(r.accept(frame(2))?.event).toMatchObject({
    kind: "ready",
    boundary: { sourceBytes: base + 33n, utf8Bytes: base + 43n },
  });
  expect(r.accept(frame(3))?.event).toEqual({
    kind: "marker",
    revision: 2n,
    sourceBytes: base + 34n,
    utf8Bytes: base + 43n,
  });
  expect(r.accept(frame(4))?.event).toMatchObject({
    kind: "data",
    sourceBytes: base + 35n,
    offset: base + 43n,
  });
  r.accept(frame(5));
  r.accept(frame(6));
  expect(r.accept(frame(7))?.event).toMatchObject({
    kind: "repair",
    boundary: { sourceBytes: base + 35n, utf8Bytes: base + 46n },
  });
  expect(r.accept(frame(8))?.event).toEqual({
    kind: "end",
    revision: 4n,
    sourceBytes: base + 35n,
    utf8Bytes: base + 46n,
  });
});
it("rejects mixed versions and validates duplicated Begin source", () => {
  expect(() => new AttachmentReceiver(generation).accept(frame(0))).toThrow(
    "version",
  );
  for (const version of [1, 3]) {
    const w = frame(0);
    w[6] = version;
    expect(() => new AttachmentV2Receiver(generation).accept(w)).toThrow(
      "version",
    );
  }
  const w = frame(0);
  w[26] = 0;
  expect(() => new AttachmentV2Receiver(generation).accept(w)).toThrow(
    "begin_source",
  );
});
it("rejects source regression independently of correct UTF8 offset and poisons epoch", () => {
  const r = new AttachmentV2Receiver(generation);
  feed(r);
  const w = frame(3);
  new DataView(w.buffer).setBigUint64(26, base + 32n, true);
  expect(() => r.accept(w)).toThrow("source_regression");
  expect(() => r.accept(frame(3))).toThrow("failed");
});
it("repair and end require unchanged source and exact display cursor", () => {
  for (const i of [5, 8]) {
    const r = new AttachmentV2Receiver(generation);
    feed(r, i);
    const w = frame(i);
    new DataView(w.buffer).setBigUint64(26, base + 36n, true);
    expect(() => r.accept(w)).toThrow("source_boundary");
  }
  const r = new AttachmentV2Receiver(generation);
  feed(r, 8);
  const w = frame(8);
  new DataView(w.buffer).setBigUint64(18, base + 47n, true);
  expect(() => r.accept(w)).toThrow("checkpoint");
});
it("stale generation cannot advance sequence or source", () => {
  const r = new AttachmentV2Receiver(generation);
  const w = frame(0);
  new DataView(w.buffer).setBigUint64(10, generation + 1n, true);
  expect(r.accept(w)).toBeNull();
  expect(r.accept(frame(0))?.sequence).toBe(0);
});
it("ACK follows completed callback for live marker/data/repair/EOF", async () => {
  const s = sink();
  const calls: string[] = [];
  s.marker = async (b) => {
    calls.push(`marker:${b.sourceBytes}`);
  };
  s.repair = async () => {
    calls.push("repair");
  };
  s.finish = async () => {
    calls.push("end");
  };
  const c = new AttachmentV2Consumer(generation, s, (_g, seq) => {
    calls.push(`ack:${seq}`);
  });
  for (const w of wires) await c.enqueue(w);
  expect(calls.indexOf("repair")).toBeLessThan(calls.indexOf("ack:7"));
  expect(calls.indexOf("end")).toBeLessThan(calls.indexOf("ack:8"));
  expect(calls.indexOf(`marker:${base + 34n}`)).toBeLessThan(
    calls.indexOf("ack:3"),
  );
  expect(c.queuedBytes).toBe(0);
});
it("abort and late renderer completion cannot send an ACK", async () => {
  let release = () => {};
  const s = sink(),
    ack = vi.fn();
  s.reset = () =>
    new Promise<void>((resolve) => {
      release = resolve;
    });
  const c = new AttachmentV2Consumer(generation, s, ack);
  await c.enqueue(frame(0));
  await c.enqueue(frame(1));
  const pending = c.enqueue(frame(2));
  const rejection = expect(pending).rejects.toThrow("cancelled");
  c.cancel();
  await rejection;
  release();
  await Promise.resolve();
  await Promise.resolve();
  expect(ack).toHaveBeenCalledTimes(2);
});
it("missing source marker support and queue overflow fail closed", async () => {
  const s = sink();
  delete s.marker;
  const c = new AttachmentV2Consumer(generation, s, () => {});
  for (const w of wires.slice(0, 3)) await c.enqueue(w);
  await expect(c.enqueue(frame(3))).rejects.toThrow("marker_unsupported");
  const small = new AttachmentV2Consumer(generation, s, () => {}, 1, 1);
  await expect(small.enqueue(frame(0))).rejects.toThrow("queue_limit");
});
