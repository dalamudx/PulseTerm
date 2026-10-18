import { readFileSync } from "node:fs";
import { expect, it, vi } from "vitest";
import { AttachmentV2Consumer, type V2Sink } from "./attachment-v2";

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
  const wire = wires[i];
  if (!wire) throw Error("fixture");
  return wire.slice();
}
function data(sequence: number, offset = base + 43n + BigInt(sequence - 4)) {
  const wire = frame(4).slice(0, 35),
    view = new DataView(wire.buffer);
  view.setUint32(2, sequence, true);
  view.setBigUint64(18, offset, true);
  view.setBigUint64(26, base + 35n + BigInt(sequence - 4), true);
  wire[34] = 97 + sequence - 4;
  return wire;
}
function marker(sequence: number, source: bigint, utf8: bigint) {
  const wire = frame(3),
    view = new DataView(wire.buffer);
  view.setUint32(2, sequence, true);
  view.setBigUint64(18, utf8, true);
  view.setBigUint64(26, source, true);
  view.setBigUint64(34, 3n, true);
  return wire;
}
function gate() {
  let release = () => {};
  const promise = new Promise<void>((r) => {
    release = r;
  });
  return { promise, release };
}
function fixture() {
  const ack = vi.fn(),
    write = vi.fn(async () => {});
  const sink: V2Sink = { reset: async () => {}, write, marker: async () => {} };
  const consumer = new AttachmentV2Consumer(generation, sink, ack);
  const bootstrap = async () => {
    for (let i = 0; i < 4; i++) await consumer.enqueue(frame(i));
    ack.mockClear();
  };
  return { consumer, sink, ack, write, bootstrap };
}
it("draws bounded adjacent Data together, preserves per-frame receipts and ACKs, and never crosses a Marker", async () => {
  const f = fixture(),
    first = gate(),
    entered = gate();
  const calls: string[] = [];
  const batch = vi.fn(async (chunks: readonly Uint8Array[]) => {
    calls.push(`draw:${chunks.map((c) => String.fromCharCode(...c)).join("")}`);
    if (batch.mock.calls.length === 1) {
      entered.release();
      await first.promise;
    }
  });
  f.sink.writeBatch = batch;
  f.sink.marker = async () => {
    calls.push("marker");
  };
  await f.bootstrap();
  calls.length = 0;
  f.ack.mockImplementation((_g: bigint, seq: number) => {
    calls.push(`ack:${seq}`);
  });
  const a = f.consumer.enqueue(data(4));
  await entered.promise;
  const pending = [
    a,
    f.consumer.enqueue(data(5)),
    f.consumer.enqueue(marker(6, base + 37n, base + 45n)),
    f.consumer.enqueue(data(7, base + 45n)),
    f.consumer.enqueue(data(8, base + 46n)),
  ];
  expect(f.consumer.queuedFrames).toBe(5);
  expect(f.ack).not.toHaveBeenCalled();
  first.release();
  const receipts = await Promise.all(pending);
  expect(receipts.map((r) => r?.sequence)).toEqual([4, 5, 6, 7, 8]);
  expect(calls).toEqual([
    "draw:a",
    "ack:4",
    "draw:b",
    "ack:5",
    "marker",
    "ack:6",
    "draw:de",
    "ack:7",
    "ack:8",
  ]);
  expect(f.consumer.queuedFrames).toBe(0);
  expect(f.consumer.queuedBytes).toBe(0);
});
it("cancels every member of an active batch and withholds all their ACKs after a late draw", async () => {
  const f = fixture(),
    first = gate(),
    second = gate(),
    entered = gate();
  let draws = 0;
  f.sink.writeBatch = async () => {
    if (++draws === 1) await first.promise;
    else {
      entered.release();
      await second.promise;
    }
  };
  await f.bootstrap();
  const a = f.consumer.enqueue(data(4)),
    b = f.consumer.enqueue(data(5)),
    c = f.consumer.enqueue(data(6));
  const rejected = Promise.all([
    expect(b).rejects.toThrow("cancelled"),
    expect(c).rejects.toThrow("cancelled"),
  ]);
  first.release();
  await a;
  await entered.promise;
  expect(f.consumer.queuedFrames).toBe(2);
  expect(f.consumer.queuedBytes).toBe(data(5).length + data(6).length);
  f.consumer.cancel();
  await rejected;
  second.release();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.ack.mock.calls.map(([, seq]) => seq)).toEqual([4]);
  expect(f.consumer.queuedFrames).toBe(0);
  expect(f.consumer.queuedBytes).toBe(0);
});
it("validates every queued cursor before drawing any member of a malformed batch", async () => {
  const f = fixture(),
    first = gate();
  const batch = vi.fn(async () => {
    await first.promise;
  });
  f.sink.writeBatch = batch;
  await f.bootstrap();
  const a = f.consumer.enqueue(data(4)),
    b = f.consumer.enqueue(data(5)),
    c = f.consumer.enqueue(data(6, base + 999n));
  const rejected = Promise.all([
    expect(b).rejects.toThrow(),
    expect(c).rejects.toThrow(),
  ]);
  first.release();
  await a;
  await rejected;
  expect(batch).toHaveBeenCalledOnce();
  expect(f.ack.mock.calls.map(([, seq]) => seq)).toEqual([4]);
  expect(f.consumer.queuedFrames).toBe(0);
  expect(f.consumer.queuedBytes).toBe(0);
});
it("releases each drawn member before a reentrant ACK replenishes a full queue", async () => {
  const confirmed = gate(),
    confirming = gate(),
    done = gate();
  const batches: number[] = [],
    jobs: Promise<unknown>[] = [],
    acked: number[] = [];
  let failure: unknown;
  const sink: V2Sink = {
    reset: async () => {},
    write: async () => {},
    ready: async () => {
      confirming.release();
      await confirmed.promise;
    },
    writeBatch: async (chunks) => {
      batches.push(chunks.length);
    },
  };
  const c = new AttachmentV2Consumer(
    generation,
    sink,
    (_g, seq) => {
      acked.push(seq);
      if (seq >= 3 && seq <= 5) {
        const p = c.enqueue(data(seq + 3, base + 43n + BigInt(seq)));
        jobs.push(p);
        void p.catch((e) => {
          failure = e;
          done.release();
        });
      }
      if (seq === 8) done.release();
    },
    3,
  );
  await c.enqueue(frame(0));
  await c.enqueue(frame(1));
  const commit = c.enqueue(frame(2));
  await confirming.promise;
  expect(c.queuedFrames).toBe(0);
  for (let seq = 3; seq <= 5; seq++)
    jobs.push(c.enqueue(data(seq, base + 43n + BigInt(seq - 3))));
  expect(c.queuedFrames).toBe(3);
  confirmed.release();
  await commit;
  await done.promise;
  await Promise.all(jobs);
  expect(failure).toBeUndefined();
  expect(batches).toEqual([3, 3]);
  expect(acked).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
  expect(c.queuedFrames).toBe(0);
  expect(c.queuedBytes).toBe(0);
});
