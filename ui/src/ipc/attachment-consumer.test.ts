import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { AttachmentConsumer, type AttachmentSink } from "./attachment-consumer";

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
      sink([]),
      () => blocked.promise,
      1,
      1024,
    );
    const first = consumer.enqueue(wireAt(0));
    const firstRejected = expect(first).rejects.toThrow("queue_limit");
    await expect(consumer.enqueue(wireAt(1))).rejects.toThrow("queue_limit");
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
