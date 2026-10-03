import { expect, it } from "vitest";
import { FrameVerifier, percentile } from "./verification";

function frame(sequence: number): Uint8Array {
  const bytes = new Uint8Array(262);
  const view = new DataView(bytes.buffer);
  bytes[0] = 1;
  view.setUint32(2, sequence, true);
  view.setFloat64(6, 100, true);
  for (let index = 8; index < 256; index++)
    bytes[index + 6] = (sequence + index) & 255;
  return bytes;
}
const config = { chunkBytes: 256, frames: 2, window: 2 };

it("checks every byte, sequence and completion", () => {
  const verifier = new FrameVerifier(config, 10);
  verifier.consume(frame(0), 112);
  expect(verifier.deliveryMs).toEqual([2]);
  expect(() => verifier.finish()).toThrow("probe.incomplete");
  expect(() => verifier.consume(frame(0), 113)).toThrow("probe.sequence");
  verifier.consume(frame(1), 114);
  verifier.finish();
  expect(() => verifier.consume(frame(2), 115)).toThrow("probe.sequence");
});

it("rejects corruption and wrong payload size", () => {
  const verifier = new FrameVerifier(config, 0);
  const bytes = frame(0);
  bytes[40] = 99;
  expect(() => verifier.consume(bytes, 100)).toThrow("probe.corrupt");
  expect(() => verifier.consume(frame(0).subarray(0, 261), 100)).toThrow(
    "probe.length",
  );
});

it("computes nearest-rank p95 without mutating the samples", () => {
  const values = [5, 1, 2, 3, 4];
  expect(percentile(values, 95)).toBe(5);
  expect(values).toEqual([5, 1, 2, 3, 4]);
  expect(percentile([], 95)).toBe(0);
});
