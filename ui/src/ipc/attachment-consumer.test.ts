import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { AttachmentConsumer, type AttachmentSink } from "./attachment-consumer";

it("reentrant ACK releases consumed wire capacity before the sender supplies its next frame", async () => {
  let finish!: () => void, fail!: (e: unknown) => void;
  const done = new Promise<void>((resolve, reject) => {
    finish = resolve;
    fail = reject;
  });
  let next = 1;
  const jobs: Promise<unknown>[] = [];
  const c = new AttachmentConsumer(
    generation,
    sink([]),
    () => {
      if (next < fixture.length) {
        const wire = fixture[next++];
        if (!wire) throw new Error("fixture");
        const p = c.enqueue(wire);
        jobs.push(p);
        void p.catch(fail);
      } else finish();
    },
    1,
  );
  jobs.push(c.enqueue(fixture[0] as Uint8Array));
  void jobs[0]?.catch(fail);
  await done;
  await Promise.all(jobs);
  expect(jobs).toHaveLength(fixture.length);
  expect(c.queuedFrames).toBe(0);
  expect(c.queuedBytes).toBe(0);
});

const fixture = readFileSync(
  new URL("../../../tests/fixtures/frames/attachment.hex", import.meta.url),
  "utf8",
)
  .trim()
  .split("\n")
  .map((line) =>
    Uint8Array.from(line.trim().split(/\s+/), (v) => Number.parseInt(v, 16)),
  );
function wireAt(index: number): Uint8Array {
  const wire = fixture[index];
  if (!wire) throw new Error("fixture");
  return wire;
}
const firstWire = wireAt(0);
const generation = new DataView(
  firstWire.buffer,
  firstWire.byteOffset,
).getBigUint64(10, true);
const text = new TextDecoder();
function repairWire(
  part: number,
  sequence: number,
  body: Uint8Array,
  offset = 10n,
): Uint8Array {
  const raw = new Uint8Array(26 + body.length);
  const v = new DataView(raw.buffer);
  raw[0] = 8;
  v.setUint32(2, sequence, true);
  raw[6] = 1;
  raw[7] = part;
  v.setBigUint64(10, generation, true);
  v.setBigUint64(18, offset, true);
  raw.set(body, 26);
  return raw;
}
function repairFrames() {
  const meta = new Uint8Array(12);
  const v = new DataView(meta.buffer);
  v.setBigUint64(0, (1n << 64n) - 4n, true);
  v.setUint32(8, 3, true);
  const seq = fixture.length;
  return [
    repairWire(9, seq, meta),
    repairWire(10, seq + 1, new TextEncoder().encode("FIX"), 0n),
    repairWire(11, seq + 2, new Uint8Array()),
  ];
}
it("repair is staged until Commit and cannot ACK before screen callback or interleave Data", async () => {
  const ack: number[] = [];
  const events: string[] = [];
  const slow = deferred();
  const target = sink(events);
  target.repair = async (boundary, bytes) => {
    expect(boundary.parsedBytes).toBe(10n);
    expect(boundary.revision).toBe((1n << 64n) - 4n);
    events.push(text.decode(bytes));
    await slow.promise;
  };
  const c = new AttachmentConsumer(generation, target, (_, seq) => {
    ack.push(seq);
  });
  for (const w of fixture) await c.enqueue(w);
  events.length = 0;
  const frames = repairFrames();
  await c.enqueue(frames[0] as Uint8Array);
  await c.enqueue(frames[1] as Uint8Array);
  expect(events).toEqual([]);
  const commit = c.enqueue(frames[2] as Uint8Array);
  await Promise.resolve();
  await Promise.resolve();
  expect(ack).not.toContain(fixture.length + 2);
  slow.resolve();
  await commit;
  expect(events).toEqual(["FIX"]);
  expect(ack).toContain(fixture.length + 2);
});
it("unsupported or cancelled repair fails closed without a commit ACK", async () => {
  for (const supported of [false, true]) {
    const ack: number[] = [];
    const slow = deferred();
    const target = sink([]);
    if (supported) target.repair = () => slow.promise;
    const c = new AttachmentConsumer(generation, target, (_, seq) => {
      ack.push(seq);
    });
    for (const w of fixture) await c.enqueue(w);
    const f = repairFrames();
    await c.enqueue(f[0] as Uint8Array);
    await c.enqueue(f[1] as Uint8Array);
    const promise = c.enqueue(f[2] as Uint8Array);
    const rejected = expect(promise).rejects.toThrow(
      supported ? "cancelled" : "repair_unsupported",
    );
    if (supported) c.cancel();
    await rejected;
    slow.resolve();
    await Promise.resolve();
    expect(ack).not.toContain(fixture.length + 2);
  }
});
it("malformed or interrupted repair never renders a partial screen", async () => {
  for (const errorPart of [5, 7, 9, 11]) {
    const target = sink([]);
    let repairs = 0;
    target.repair = async () => {
      repairs++;
    };
    const c = new AttachmentConsumer(generation, target, () => {});
    for (const w of fixture) await c.enqueue(w);
    const f = repairFrames();
    await c.enqueue(f[0] as Uint8Array);
    const bad =
      errorPart === 9
        ? repairWire(9, fixture.length + 1, new Uint8Array(12))
        : repairWire(errorPart, fixture.length + 1, new Uint8Array());
    await expect(c.enqueue(bad)).rejects.toThrow();
    expect(repairs).toBe(0);
  }
});

function deferred() {
  let resolve = () => {};
  const promise = new Promise<void>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function sink(events: string[]): AttachmentSink {
  return {
    async reset() {
      events.push("reset");
    },
    async write(bytes) {
      events.push(text.decode(bytes));
    },
  };
}
function endWire(sequence: number): Uint8Array {
  const wire = checkpointWire(sequence);
  wire[7] = 7;
  return wire;
}
it("normal End waits for decoder/render completion before ACK and rejects later frames", async () => {
  const ack: number[] = [];
  const target = sink([]);
  const pending = deferred();
  target.finish = async (revision, offset) => {
    expect(offset).toBe(10n);
    expect(revision).toBe((1n << 64n) - 4n);
    await pending.promise;
  };
  const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
    ack.push(seq);
  });
  for (const wire of fixture) await consumer.enqueue(wire);
  const request = consumer.enqueue(endWire(fixture.length));
  await Promise.resolve();
  expect(ack).not.toContain(fixture.length);
  pending.resolve();
  await request;
  expect(ack.at(-1)).toBe(fixture.length);
  await expect(
    consumer.enqueue(checkpointWire(fixture.length + 1)),
  ).rejects.toThrow("checkpoint");
});
it("missing EOF sink fails closed without claiming End ACK", async () => {
  const ack: number[] = [];
  const consumer = new AttachmentConsumer(generation, sink([]), (_, seq) => {
    ack.push(seq);
  });
  for (const wire of fixture) await consumer.enqueue(wire);
  await expect(consumer.enqueue(endWire(fixture.length))).rejects.toThrow(
    "end_unsupported",
  );
  expect(ack).not.toContain(fixture.length);
});
it("cancelling in-flight EOF never sends a late ACK", async () => {
  const ack: number[] = [];
  const target = sink([]);
  const pending = deferred();
  target.finish = () => pending.promise;
  const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
    ack.push(seq);
  });
  for (const wire of fixture) await consumer.enqueue(wire);
  const request = consumer.enqueue(endWire(fixture.length));
  await Promise.resolve();
  consumer.cancel();
  await expect(request).rejects.toThrow();
  pending.resolve();
  await Promise.resolve();
  expect(ack).not.toContain(fixture.length);
});
function markerWire(sequence: number): Uint8Array {
  const wire = checkpointWire(sequence);
  wire[7] = 8;
  return wire;
}
it("pure Marker waits for observation before ACK without writing or flushing", async () => {
  const events: string[] = [];
  const ack: number[] = [];
  const pending = deferred();
  const target = sink(events);
  target.checkpoint = async () => {
    throw new Error("must not flush");
  };
  target.marker = async (revision, offset) => {
    expect(revision).toBe((1n << 64n) - 4n);
    expect(offset).toBe(10n);
    await pending.promise;
  };
  const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
    ack.push(seq);
  });
  for (const frame of fixture) await consumer.enqueue(frame);
  const before = events.slice();
  const request = consumer.enqueue(markerWire(fixture.length));
  await Promise.resolve();
  expect(ack).not.toContain(fixture.length);
  pending.resolve();
  const receipt = await request;
  expect(receipt?.event.kind).toBe("marker");
  expect(events).toEqual(before);
  expect(ack.at(-1)).toBe(fixture.length);
});
it("unsupported or cancelled Marker cannot claim an ACK", async () => {
  for (const cancel of [false, true]) {
    const ack: number[] = [];
    const target = sink([]);
    const pending = deferred();
    if (cancel) target.marker = () => pending.promise;
    const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
      ack.push(seq);
    });
    for (const frame of fixture) await consumer.enqueue(frame);
    const request = consumer.enqueue(markerWire(fixture.length));
    if (cancel) consumer.cancel();
    await expect(request).rejects.toThrow();
    pending.resolve();
    await Promise.resolve();
    expect(ack).not.toContain(fixture.length);
  }
});
function checkpointWire(sequence: number): Uint8Array {
  const wire = new Uint8Array(34);
  const view = new DataView(wire.buffer);
  wire[0] = 8;
  view.setUint32(2, sequence, true);
  wire[6] = 1;
  wire[7] = 6;
  view.setBigUint64(10, generation, true);
  view.setBigUint64(18, 10n, true);
  view.setBigUint64(26, (1n << 64n) - 4n, true);
  return wire;
}
it("ordered checkpoint waits for sink state handling before ACK", async () => {
  const events: string[] = [];
  const ack: number[] = [];
  const pending = deferred();
  const target = sink(events);
  target.checkpoint = async (revision, offset) => {
    expect(revision).toBe((1n << 64n) - 4n);
    expect(offset).toBe(10n);
    await pending.promise;
  };
  const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
    ack.push(seq);
  });
  for (const frame of fixture) await consumer.enqueue(frame);
  const request = consumer.enqueue(checkpointWire(fixture.length));
  await Promise.resolve();
  expect(ack).not.toContain(fixture.length);
  pending.resolve();
  await request;
  expect(ack.at(-1)).toBe(fixture.length);
});
it("unsupported checkpoint cancels without claiming state ACK", async () => {
  const ack: number[] = [];
  const consumer = new AttachmentConsumer(generation, sink([]), (_, seq) => {
    ack.push(seq);
  });
  for (const wire of fixture) await consumer.enqueue(wire);
  await expect(
    consumer.enqueue(checkpointWire(fixture.length)),
  ).rejects.toThrow("checkpoint_unsupported");
  expect(ack).not.toContain(fixture.length);
});
describe("ordered attachment consumption", () => {
  it("never renders staged fragments and ACKs ready/data only after writes", async () => {
    const events: string[] = [];
    const ack: number[] = [];
    const slow = deferred();
    let commit = -1;
    const target = sink(events);
    const write = target.write;
    target.write = async (bytes, signal) => {
      await write(bytes, signal);
      if (text.decode(bytes).includes("SCREEN")) await slow.promise;
    };
    const consumer = new AttachmentConsumer(
      generation,
      target,
      (_, sequence) => {
        ack.push(sequence);
      },
    );
    for (const wire of fixture.slice(0, -1)) {
      const part = wire[7];
      if (part === 4) {
        commit = new DataView(wire.buffer, wire.byteOffset).getUint32(2, true);
        const pending = consumer.enqueue(wire);
        await Promise.resolve();
        await Promise.resolve();
        expect(ack).not.toContain(commit);
        slow.resolve();
        await pending;
      } else {
        await consumer.enqueue(wire);
        expect(events).toEqual([]);
      }
    }
    expect(events).toEqual(["reset", "older\r\n", "SCREEN"]);
    expect(ack).toContain(commit);
    await consumer.enqueue(fixture.at(-1) as Uint8Array);
    expect(events.at(-1)).toBe("Z");
    expect(consumer.queuedBytes).toBe(0);
  });
  it("cancellation of a pending render prevents a late ACK and rejects queued data", async () => {
    const slow = deferred();
    const ack: number[] = [];
    const target: AttachmentSink = {
      reset: () => slow.promise,
      async write() {},
    };
    const consumer = new AttachmentConsumer(generation, target, (_, seq) => {
      ack.push(seq);
    });
    for (const wire of fixture.slice(0, -2)) await consumer.enqueue(wire);
    const commit = consumer.enqueue(fixture.at(-2) as Uint8Array);
    const data = consumer.enqueue(fixture.at(-1) as Uint8Array);
    const commitRejected = expect(commit).rejects.toThrow("cancelled");
    const dataRejected = expect(data).rejects.toThrow("cancelled");
    consumer.cancel();
    await Promise.all([commitRejected, dataRejected]);
    slow.resolve();
    await Promise.resolve();
    await Promise.resolve();
    expect(ack).toHaveLength(fixture.length - 2);
    expect(consumer.queuedFrames).toBe(0);
  });
  it("counts active frames and cancels instead of dropping on queue overload", async () => {
    const blocked = deferred();
    const consumer = new AttachmentConsumer(
      generation,
      { reset: () => blocked.promise, async write() {} },
      () => {},
      1,
      1024,
    );
    for (const wire of fixture.slice(0, -2)) await consumer.enqueue(wire);
    const first = consumer.enqueue(fixture.at(-2) as Uint8Array);
    const firstRejected = expect(first).rejects.toThrow("queue_limit");
    await expect(
      consumer.enqueue(fixture.at(-1) as Uint8Array),
    ).rejects.toThrow("queue_limit");
    await firstRejected;
    blocked.resolve();
    await Promise.resolve();
    await expect(consumer.enqueue(wireAt(0))).rejects.toThrow("queue_limit");
  });
  it("does not reset or ACK stale-generation frames", async () => {
    const events: string[] = [];
    const ack: number[] = [];
    const consumer = new AttachmentConsumer(
      generation + 1n,
      sink(events),
      (_, seq) => {
        ack.push(seq);
      },
    );
    for (const wire of fixture) expect(await consumer.enqueue(wire)).toBeNull();
    expect(events).toEqual([]);
    expect(ack).toEqual([]);
  });
});
