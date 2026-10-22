import { expect, it } from "vitest";
import type { V2Receipt } from "../../ipc/attachment-v2";
import { TerminalOutputTiming } from "./terminal-output-timing";

const base = (1n << 60n) + 7n;
function fixture() {
  let now = 10;
  const probe = new TerminalOutputTiming(() => now++);
  probe.ready(base);
  return probe;
}
function batch(probe: TerminalOutputTiming, bytes = 4) {
  const span = probe.begin(bytes);
  probe.mark(span, "parsedMs");
  probe.mark(span, "preparedMs");
  probe.opportunity();
  probe.opportunity();
  probe.mark(span, "fencedMs");
  probe.complete(span);
  return span;
}
function frame(sequence: number, offset: bigint, bytes = 2): V2Receipt {
  return {
    generation: 9n,
    sequence,
    event: {
      kind: "data",
      offset,
      sourceBytes: offset + 1000n,
      bytes: new Uint8Array(bytes).fill(97),
    },
  };
}
it("keeps exact large UTF-8 and source coordinates distinct, joining merged frames to one span", () => {
  const probe = fixture();
  batch(probe);
  probe.receipt(frame(8, base), 5);
  probe.receipt(frame(9, base + 2n), 6);
  const result = probe.snapshot();
  expect(result.batches).toHaveLength(1);
  expect(result.frames.map((f) => f.batch)).toEqual([1, 1]);
  expect(result.frames[1]).toMatchObject({
    offset: (base + 2n).toString(),
    end: (base + 4n).toString(),
    sourceBytes: (base + 1002n).toString(),
  });
  expect(result.batches[0]).toMatchObject({
    startedMs: 10,
    parsedMs: 11,
    preparedMs: 12,
    firstFrameMs: 13,
    lastFrameMs: 14,
    frameCount: 2,
    fencedMs: 15,
    verifiedMs: 16,
  });
  expect(result.frames.map((f) => f.acknowledgedMs)).toEqual([17, 18]);
  expect(JSON.stringify(result)).not.toContain('"bytes"');
});
it("ignores capture/control frames, retains no terminal content and snapshots by value", () => {
  const probe = fixture();
  batch(probe);
  const receipt = frame(8, base, 4);
  probe.receipt(receipt, 5);
  if (receipt.event.kind === "data") receipt.event.bytes.fill(255);
  probe.receipt(null, 0);
  probe.receipt({ generation: 9n, sequence: 9, event: { kind: "staged" } }, 0);
  const result = probe.snapshot();
  expect(result.frames).toHaveLength(1);
  const value = result.batches[0];
  if (!value) throw new Error("fixture");
  value.parsedMs = 99;
  expect(probe.snapshot().batches[0]?.parsedMs).toBe(11);
});
it("does not record before capture, and clears all metadata on retirement", () => {
  const probe = new TerminalOutputTiming();
  expect(probe.begin(4)).toBeUndefined();
  probe.ready(base);
  batch(probe);
  probe.dispose();
  expect(probe.begin(4)).toBeUndefined();
  expect(() => probe.snapshot()).toThrow("unavailable");
});
it("caps batches and reports overflow instead of silently dropping tail work", () => {
  const probe = fixture();
  for (let i = 0; i < 256; i++) batch(probe, 1);
  expect(probe.begin(1)).toBeUndefined();
  expect(probe.snapshot()).toMatchObject({ overflow: true });
  expect(probe.snapshot().batches).toHaveLength(256);
});
it("caps retained frame metadata even when one batch contains many frames", () => {
  const probe = fixture();
  batch(probe, 257);
  for (let i = 0; i < 257; i++) probe.receipt(frame(i, base + BigInt(i), 1), 0);
  expect(probe.snapshot().frames).toHaveLength(256);
  expect(probe.snapshot().overflow).toBe(true);
});
it("incomplete or uncorrelated spans cannot be exported as complete", () => {
  const probe = fixture();
  probe.begin(4);
  expect(() => probe.snapshot()).toThrow("unavailable");
  const other = fixture();
  batch(other);
  other.receipt(frame(1, base + 4n), 0);
  expect(other.snapshot().overflow).toBe(true);
});

it("records each ACK start before completion and releases pending metadata by sequence", () => {
  const probe = fixture();
  batch(probe);
  probe.acknowledging(8);
  probe.acknowledging(9);
  probe.receipt(frame(8, base), 5);
  probe.receipt(frame(9, base + 2n), 6);
  const frames = probe.snapshot().frames;
  expect(frames.map((f) => f.acknowledgeStartedMs)).toEqual([17, 18]);
  expect(frames.map((f) => f.acknowledgedMs)).toEqual([19, 20]);
  probe.acknowledging(8); // Completed metadata has been released, not retained forever.
  expect(probe.snapshot().overflow).toBe(false);
  probe.dispose();
  probe.acknowledging(10);
  expect(() => probe.snapshot()).toThrow("unavailable");
});
it("caps pending ACK metadata and rejects repeated starts instead of overwriting history", () => {
  const probe = fixture();
  batch(probe);
  for (let i = 0; i <= 256; i++) probe.acknowledging(i);
  expect(probe.snapshot().overflow).toBe(true);
  const duplicate = fixture();
  batch(duplicate);
  duplicate.acknowledging(8);
  duplicate.acknowledging(8);
  expect(duplicate.snapshot().overflow).toBe(true);
});

it("only a trusted fixed physical chord selects task completion, with per-batch identity", () => {
  const probe = fixture();
  const event = {
    isTrusted: true,
    code: "F9",
    ctrlKey: true,
    shiftKey: true,
    altKey: false,
    metaKey: false,
    repeat: false,
    isComposing: false,
    preventDefault() {},
    stopImmediatePropagation() {},
  } as KeyboardEvent;
  probe.control({ ...event, isTrusted: false });
  expect(probe.yieldAfterFence).toBe(false);
  const active = probe.begin(1);
  probe.control(event);
  expect(probe.yieldAfterFence).toBe(false);
  probe.complete(active);
  probe.control(event);
  expect(probe.yieldAfterFence).toBe(true);
  batch(probe, 1);
  expect(probe.snapshot().batches.map((row) => row.postFrameTask)).toEqual([
    false,
    true,
  ]);
  probe.dispose();
  probe.yieldAfterFence = false;
  probe.control(event);
  expect(probe.yieldAfterFence).toBe(false);
});
