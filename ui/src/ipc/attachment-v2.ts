// Dual-coordinate v2. v1 is reused only as a private structural transaction engine.
import {
  type AttachBoundary,
  type AttachmentEvent,
  AttachmentReceiver,
  MAX_FRAGMENT,
} from "./attachment";
import { decodeFrame } from "./frames";
export const V2_ENVELOPE_LENGTH = 28;
export interface V2Boundary {
  columns: number;
  rows: number;
  revision: bigint;
  sourceBytes: bigint;
  utf8Bytes: bigint;
}
export type V2Event =
  | { kind: "staged" }
  | {
      kind: "ready";
      boundary: V2Boundary;
      history: Uint8Array;
      restore: Uint8Array;
    }
  | { kind: "repair"; boundary: V2Boundary; bytes: Uint8Array }
  | { kind: "data"; bytes: Uint8Array; offset: bigint; sourceBytes: bigint }
  | {
      kind: "checkpoint" | "marker" | "end";
      revision: bigint;
      sourceBytes: bigint;
      utf8Bytes: bigint;
    };
export interface V2Receipt {
  generation: bigint;
  sequence: number;
  event: V2Event;
}
export class AttachmentV2Receiver {
  private readonly engine: AttachmentReceiver;
  private source = 0n;
  private failed = false;
  private begun = false;
  constructor(readonly generation: bigint) {
    this.engine = new AttachmentReceiver(generation);
  }
  accept(wire: Uint8Array): V2Receipt | null {
    if (this.failed) throw new Error("attach2.failed");
    try {
      return this.consume(wire);
    } catch (error) {
      this.failed = true;
      throw error;
    }
  }
  private consume(wire: Uint8Array): V2Receipt | null {
    const frame = decodeFrame(wire);
    const p = frame.payload;
    if (
      frame.kind !== 8 ||
      p.length < V2_ENVELOPE_LENGTH ||
      p.length > V2_ENVELOPE_LENGTH + MAX_FRAGMENT
    )
      throw new Error("attach2.length_or_kind");
    const meta = new DataView(p.buffer, p.byteOffset, p.byteLength);
    if (p[0] !== 2 || meta.getUint16(2, true) !== 0)
      throw new Error("attach2.version_or_flags");
    const generation = meta.getBigUint64(4, true);
    if (generation === 0n) throw new Error("attach2.generation");
    if (generation !== this.generation) return null;
    const part = meta.getUint8(1),
      source = meta.getBigUint64(20, true);
    let body = p.subarray(V2_ENVELOPE_LENGTH);
    if (part === 1) {
      if (this.begun || body.length !== 36) throw new Error("attach2.begin");
      const b = new DataView(body.buffer, body.byteOffset, body.byteLength);
      if (b.getBigUint64(12, true) !== source)
        throw new Error("attach2.begin_source");
      const translated = new Uint8Array(28);
      translated.set(body.subarray(0, 12));
      translated.set(body.subarray(20, 36), 12);
      body = translated;
    } else {
      if (!this.begun) throw new Error("attach2.before_begin");
      if (source < this.source) throw new Error("attach2.source_regression");
      if (part !== 5 && part !== 8 && source !== this.source)
        throw new Error("attach2.source_boundary");
    }
    // v1 receives only a private UTF-8 cursor. Never expose parsedBytes as raw source.
    const mapped = new Uint8Array(26 + body.length);
    mapped.set(wire.subarray(0, 6));
    mapped.set(p.subarray(0, 20), 6);
    mapped[6] = 1;
    mapped.set(body, 26);
    const receipt = this.engine.accept(mapped);
    if (!receipt) throw new Error("attach2.engine_generation");
    this.source = source;
    this.begun = true;
    const boundary = (b: AttachBoundary): V2Boundary => ({
      columns: b.columns,
      rows: b.rows,
      revision: b.revision,
      sourceBytes: source,
      utf8Bytes: b.parsedBytes,
    });
    const original: AttachmentEvent = receipt.event;
    let event: V2Event;
    switch (original.kind) {
      case "ready":
        event = { ...original, boundary: boundary(original.boundary) };
        break;
      case "repair":
        event = { ...original, boundary: boundary(original.boundary) };
        break;
      case "data":
        event = { ...original, sourceBytes: source };
        break;
      case "checkpoint":
      case "marker":
      case "end":
        event = {
          kind: original.kind,
          revision: original.revision,
          sourceBytes: source,
          utf8Bytes: original.parsedBytes,
        };
        break;
      default:
        event = original;
    }
    return {
      generation: receipt.generation,
      sequence: receipt.sequence,
      event,
    };
  }
}
export interface V2Sink {
  /** After Commit ACK but before consuming queued Data: confirm the captured
   * image, not latest-state freshness. Failure/cancellation retires the stream. */
  ready?(boundary: V2Boundary, signal: AbortSignal): Promise<void>;
  reset(boundary: V2Boundary, signal: AbortSignal): Promise<void>;
  write(bytes: Uint8Array, signal: AbortSignal): Promise<void>;
  /** Optional bounded live-data batching; resolves only after the shared draw. */
  writeBatch?(
    chunks: readonly Uint8Array[],
    signal: AbortSignal,
  ): Promise<void>;
  repair?(
    boundary: V2Boundary,
    bytes: Uint8Array,
    signal: AbortSignal,
  ): Promise<void>;
  marker?(
    boundary: Extract<V2Event, { kind: "checkpoint" | "marker" | "end" }>,
    signal: AbortSignal,
  ): Promise<void>;
  checkpoint?(
    boundary: Extract<V2Event, { kind: "checkpoint" | "marker" | "end" }>,
    signal: AbortSignal,
  ): Promise<void>;
  finish?(
    boundary: Extract<V2Event, { kind: "checkpoint" | "marker" | "end" }>,
    signal: AbortSignal,
  ): Promise<void>;
}
type Job = {
  wire: Uint8Array;
  resolve: (r: V2Receipt | null) => void;
  reject: (e: Error) => void;
};
/** One ordered queue including the active frame; ACK is after completed rendering.
 * Abort discards this renderer generation, never flushes a decoder as if genuine EOF. */
export class AttachmentV2Consumer {
  private readonly receiver: AttachmentV2Receiver;
  private readonly abort = new AbortController();
  private readonly queue: Job[] = [];
  private active: Job[] = [];
  private running = false;
  private frames = 0;
  private bytes = 0;
  constructor(
    generation: bigint,
    private readonly sink: V2Sink,
    private readonly ack: (
      generation: bigint,
      sequence: number,
    ) => void | Promise<void>,
    private readonly maxFrames = 8,
    private readonly maxBytes = 8 * (6 + V2_ENVELOPE_LENGTH + MAX_FRAGMENT),
  ) {
    if (
      !Number.isSafeInteger(maxFrames) ||
      maxFrames < 1 ||
      maxFrames > 1024 ||
      !Number.isSafeInteger(maxBytes) ||
      maxBytes < 1
    )
      throw new Error("attach2.queue_limits");
    this.receiver = new AttachmentV2Receiver(generation);
  }
  get queuedBytes() {
    return this.bytes;
  }
  get queuedFrames() {
    return this.frames;
  }
  enqueue(wire: Uint8Array): Promise<V2Receipt | null> {
    if (this.abort.signal.aborted) return Promise.reject(this.reason());
    if (
      wire.length < 6 + V2_ENVELOPE_LENGTH ||
      wire.length > 6 + V2_ENVELOPE_LENGTH + MAX_FRAGMENT ||
      this.frames >= this.maxFrames ||
      wire.length > this.maxBytes - this.bytes
    ) {
      const e = new Error("attach2.queue_limit");
      this.cancel(e);
      return Promise.reject(e);
    }
    this.frames++;
    this.bytes += wire.length;
    const promise = new Promise<V2Receipt | null>((resolve, reject) =>
      this.queue.push({ wire: wire.slice(), resolve, reject }),
    );
    if (!this.running) void this.drain();
    return promise;
  }
  cancel(error = new Error("attach2.cancelled")) {
    if (this.abort.signal.aborted) return;
    this.abort.abort(error);
    for (const job of this.active) job.reject(error);
    for (const job of this.queue.splice(0)) {
      this.frames--;
      this.bytes -= job.wire.length;
      job.reject(error);
    }
  }
  private reason(): Error {
    return this.abort.signal.reason instanceof Error
      ? this.abort.signal.reason
      : new Error("attach2.cancelled");
  }
  private async wait(operation: Promise<void>) {
    if (this.abort.signal.aborted) throw this.reason();
    await new Promise<void>((resolve, reject) => {
      const onAbort = () => reject(this.reason());
      this.abort.signal.addEventListener("abort", onAbort, { once: true });
      operation.then(
        () => {
          this.abort.signal.removeEventListener("abort", onAbort);
          resolve();
        },
        (e) => {
          this.abort.signal.removeEventListener("abort", onAbort);
          reject(e);
        },
      );
    });
    if (this.abort.signal.aborted) throw this.reason();
  }
  /** Rendering is complete: discard owned wire BEFORE ACK can release sender
   * credit and synchronously enqueue its replacement. Keep the active promise
   * cancellable while ACK/Commit confirmation waits, but never double-charge it. */
  private releaseWire(job: Job): void {
    if (!job.wire.length) return;
    this.frames--;
    this.bytes -= job.wire.length;
    job.wire = new Uint8Array(0);
  }
  private async drain() {
    this.running = true;
    try {
      while (!this.abort.signal.aborted) {
        const job = this.queue.shift();
        if (!job) break;
        this.active = [job];
        try {
          const receipt = this.receiver.accept(job.wire);
          if (receipt) {
            const e = receipt.event,
              s = this.abort.signal;
            if (e.kind === "ready") {
              await this.wait(this.sink.reset(e.boundary, s));
              if (e.history.length)
                await this.wait(this.sink.write(e.history, s));
              await this.wait(this.sink.write(e.restore, s));
            } else if (e.kind === "data") {
              if (this.sink.writeBatch) {
                const receipts = [receipt],
                  chunks = [e.bytes];
                // Peek only to select adjacent live data. The receiver still
                // validates every sequence, source/display cursor and envelope.
                while (
                  this.queue[0]?.wire[0] === 8 &&
                  this.queue[0]?.wire[6] === 2 &&
                  this.queue[0]?.wire[7] === 5 &&
                  new DataView(this.queue[0].wire.buffer).getBigUint64(
                    10,
                    true,
                  ) === receipt.generation
                ) {
                  const next = this.queue.shift();
                  if (!next) break;
                  this.active.push(next);
                  const following = this.receiver.accept(next.wire);
                  if (following?.event.kind !== "data")
                    throw new Error("attach2.batch_data");
                  receipts.push(following);
                  chunks.push(following.event.bytes);
                }
                await this.wait(this.sink.writeBatch(chunks, s));
                for (const [index, owned] of this.active.entries()) {
                  const drawn = receipts[index];
                  if (!drawn) throw new Error("attach2.batch_receipt");
                  this.releaseWire(owned);
                  await this.wait(
                    Promise.resolve(this.ack(drawn.generation, drawn.sequence)),
                  );
                  owned.resolve(drawn);
                }
                continue;
              }
              await this.wait(this.sink.write(e.bytes, s));
            } else if (e.kind === "repair") {
              if (!this.sink.repair)
                throw new Error("attach2.repair_unsupported");
              await this.wait(this.sink.repair(e.boundary, e.bytes, s));
            } else if (e.kind !== "staged") {
              const callback =
                e.kind === "end"
                  ? this.sink.finish
                  : e.kind === "marker"
                    ? this.sink.marker
                    : this.sink.checkpoint;
              if (!callback) throw new Error(`attach2.${e.kind}_unsupported`);
              await this.wait(callback.call(this.sink, e, s));
            }
            this.releaseWire(job);
            await this.wait(
              Promise.resolve(this.ack(receipt.generation, receipt.sequence)),
            );
            if (e.kind === "ready" && this.sink.ready)
              await this.wait(this.sink.ready(e.boundary, s));
          }
          job.resolve(receipt);
        } catch (error) {
          const e =
            error instanceof Error ? error : new Error("attach2.consume");
          job.reject(e);
          this.cancel(e);
        } finally {
          for (const owned of this.active) this.releaseWire(owned);
          this.active = [];
        }
      }
    } finally {
      this.running = false;
    }
  }
}
