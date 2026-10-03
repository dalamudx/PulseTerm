// Ordered rendering and ACK supervision, not a Tauri listener or session lease.

import {
  type AttachBoundary,
  type AttachmentReceipt,
  AttachmentReceiver,
  ENVELOPE_LENGTH,
  MAX_FRAGMENT,
} from "./attachment";
import { HEADER_LENGTH } from "./frames";
/** A sink owns one renderer generation. After abort discard that surface; never
 * reuse it while an old native/xterm write may still complete. All operations
 * must observe signal; split large replays into bounded callback-driven writes. */
export interface AttachmentSink {
  /** Process same-boundary state before ACK; absence fails checkpoint frames. */
  checkpoint?(
    revision: bigint,
    parsedBytes: bigint,
    signal: AbortSignal,
  ): Promise<void>;
  /** Genuine EOF: flush the stream decoder and finish rendering before ACK.
   * Cancellation or resize is NOT EOF; never finish the old decoder for those. */
  finish?(
    revision: bigint,
    parsedBytes: bigint,
    signal: AbortSignal,
  ): Promise<void>;
  reset(boundary: AttachBoundary, signal: AbortSignal): Promise<void>;
  write(bytes: Uint8Array, signal: AbortSignal): Promise<void>;
}
type Job = {
  wire: Uint8Array;
  resolve: (receipt: AttachmentReceipt | null) => void;
  reject: (error: Error) => void;
};
/** Queue includes the active frame; overload cancels the generation rather than
 * drop/skip bytes. Staging is <=5 MiB in Receiver, queued wires have separate caps.
 * Ready is reset -> history -> screen, and ACK waits for the render callbacks.
 * A fresh sink reset is required: Restore must be a screen-only replay, not RIS. */
export class AttachmentConsumer {
  private readonly receiver: AttachmentReceiver;
  private readonly abort = new AbortController();
  private readonly queue: Job[] = [];
  private active: Job | undefined;
  private running = false;
  private frames = 0;
  private bytes = 0;
  constructor(
    generation: bigint,
    private readonly sink: AttachmentSink,
    private readonly ack: (
      generation: bigint,
      sequence: number,
    ) => void | Promise<void>,
    private readonly maxFrames = 8,
    private readonly maxBytes = 8 *
      (HEADER_LENGTH + ENVELOPE_LENGTH + MAX_FRAGMENT),
  ) {
    if (
      !Number.isSafeInteger(maxFrames) ||
      maxFrames < 1 ||
      !Number.isSafeInteger(maxBytes) ||
      maxBytes < 1
    )
      throw new Error("attach.queue_limits");
    this.receiver = new AttachmentReceiver(generation);
  }
  get queuedFrames() {
    return this.frames;
  }
  get queuedBytes() {
    return this.bytes;
  }
  enqueue(wire: Uint8Array): Promise<AttachmentReceipt | null> {
    if (this.abort.signal.aborted) return Promise.reject(this.reason());
    if (
      wire.length < HEADER_LENGTH + ENVELOPE_LENGTH ||
      wire.length > HEADER_LENGTH + ENVELOPE_LENGTH + MAX_FRAGMENT ||
      this.frames >= this.maxFrames ||
      wire.length > this.maxBytes - this.bytes
    ) {
      const error = new Error("attach.queue_limit");
      this.cancel(error);
      return Promise.reject(error);
    }
    this.frames++;
    this.bytes += wire.length;
    const promise = new Promise<AttachmentReceipt | null>((resolve, reject) => {
      this.queue.push({ wire: wire.slice(), resolve, reject });
    });
    if (!this.running) void this.drain();
    return promise;
  }
  cancel(error = new Error("attach.cancelled")) {
    if (this.abort.signal.aborted) return;
    this.abort.abort(error);
    this.active?.reject(error);
    for (const job of this.queue.splice(0)) {
      this.frames--;
      this.bytes -= job.wire.length;
      job.reject(error);
    }
  }
  private reason(): Error {
    const reason: unknown = this.abort.signal.reason;
    return reason instanceof Error ? reason : new Error("attach.cancelled");
  }
  private async wait(operation: Promise<void>): Promise<void> {
    if (this.abort.signal.aborted) throw this.reason();
    await new Promise<void>((resolve, reject) => {
      const aborted = () => reject(this.reason());
      this.abort.signal.addEventListener("abort", aborted, { once: true });
      operation.then(
        () => {
          this.abort.signal.removeEventListener("abort", aborted);
          resolve();
        },
        (error) => {
          this.abort.signal.removeEventListener("abort", aborted);
          reject(error);
        },
      );
    });
    if (this.abort.signal.aborted) throw this.reason();
  }
  private async drain() {
    this.running = true;
    try {
      while (!this.abort.signal.aborted) {
        const job = this.queue.shift();
        if (job === undefined) break;
        this.active = job;
        try {
          const receipt = this.receiver.accept(job.wire);
          if (receipt !== null) {
            const event = receipt.event;
            if (event.kind === "ready") {
              await this.wait(
                this.sink.reset(event.boundary, this.abort.signal),
              );
              if (event.history.length > 0)
                await this.wait(
                  this.sink.write(event.history, this.abort.signal),
                );
              await this.wait(
                this.sink.write(event.restore, this.abort.signal),
              );
            } else if (event.kind === "data") {
              await this.wait(this.sink.write(event.bytes, this.abort.signal));
            } else if (event.kind === "end") {
              if (!this.sink.finish) throw new Error("attach.end_unsupported");
              await this.wait(
                this.sink.finish(
                  event.revision,
                  event.parsedBytes,
                  this.abort.signal,
                ),
              );
            } else if (event.kind === "checkpoint") {
              if (!this.sink.checkpoint)
                throw new Error("attach.checkpoint_unsupported");
              await this.wait(
                this.sink.checkpoint(
                  event.revision,
                  event.parsedBytes,
                  this.abort.signal,
                ),
              );
            }
            await this.wait(
              Promise.resolve(this.ack(receipt.generation, receipt.sequence)),
            );
          }
          job.resolve(receipt);
        } catch (error) {
          const reason =
            error instanceof Error ? error : new Error("attach.consume");
          job.reject(reason);
          this.cancel(reason);
        } finally {
          this.frames--;
          this.bytes -= job.wire.length;
          this.active = undefined;
        }
      }
    } finally {
      this.running = false;
    }
  }
}
