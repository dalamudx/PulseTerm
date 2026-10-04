import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import type { AttachmentSink } from "./attachment-consumer";
import {
  createProbeConsumer,
  probeAttachmentEnvelope,
} from "./probe-attachment-codec";

const generation = (1n << 60n) + 7n;
const base = 1n << 60n;
const fixture = readFileSync(
  new URL("../../../tests/fixtures/frames/attachment-v2.hex", import.meta.url),
  "utf8",
)
  .trim()
  .split("\n")
  .map((line) =>
    Uint8Array.from(line.split(/\s+/), (n) => Number.parseInt(n, 16)),
  );
function frame(i: number) {
  const f = fixture[i];
  if (!f) throw new Error("fixture");
  return f.slice();
}
function sink(): AttachmentSink {
  return {
    reset: vi.fn(async () => {}),
    write: vi.fn(async () => {}),
    marker: vi.fn(async () => {}),
  };
}
it("explicit negotiated mode refuses mixed versions and malformed envelope before routing", () => {
  const payload = frame(0).subarray(6);
  expect(probeAttachmentEnvelope(payload, 2).generation).toBe(generation);
  expect(() => probeAttachmentEnvelope(payload, 1)).toThrow("version");
  const v1 = payload.slice();
  v1[0] = 1;
  expect(() => probeAttachmentEnvelope(v1, 2)).toThrow("version");
  expect(() => probeAttachmentEnvelope(new Uint8Array(27), 2)).toThrow(
    "length",
  );
  const bad = payload.slice();
  bad[2] = 1;
  expect(() => probeAttachmentEnvelope(bad, 2)).toThrow("flags");
});
it("the fixed UTF8 shim rejects unequal bootstrap before reset and Commit ACK", async () => {
  const s = sink(),
    ack = vi.fn();
  const c = createProbeConsumer(2, generation, s, ack);
  await c.enqueue(frame(0));
  await c.enqueue(frame(1));
  await expect(c.enqueue(frame(2))).rejects.toThrow("requires_utf8");
  expect(s.reset).not.toHaveBeenCalled();
  expect(ack).toHaveBeenCalledTimes(2);
});
it("UTF8 equal-coordinate bootstrap renders then ACKs and preserves existing boundary type", async () => {
  const s = sink(),
    ack = vi.fn();
  const c = createProbeConsumer(2, generation, s, ack);
  const first = frame(0);
  new DataView(first.buffer).setBigUint64(54, base + 33n, true);
  await c.enqueue(first);
  await c.enqueue(frame(1));
  const r = await c.enqueue(frame(2));
  expect(r?.event).toMatchObject({
    kind: "ready",
    boundary: { parsedBytes: base + 33n },
  });
  expect(s.reset).toHaveBeenCalledTimes(1);
  expect(s.write).toHaveBeenCalledTimes(1);
  expect(ack).toHaveBeenCalledTimes(3);
  const pending = frame(3);
  new DataView(pending.buffer).setBigUint64(18, base + 33n, true);
  await expect(c.enqueue(pending)).rejects.toThrow("requires_utf8");
  expect(s.marker).not.toHaveBeenCalled();
  expect(ack).toHaveBeenCalledTimes(3);
});
