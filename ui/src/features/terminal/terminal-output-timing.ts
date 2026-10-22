import type { V2Receipt } from "../../ipc/attachment-v2";

export interface OutputBatchTiming {
  id: number;
  offset: string;
  end: string;
  startedMs: number;
  parsedMs: number;
  preparedMs: number;
  firstFrameMs: number;
  lastFrameMs: number;
  frameCount: number;
  fencedMs: number;
  verifiedMs: number;
  postFrameTask: boolean;
}
export interface OutputFrameTiming {
  sequence: number;
  offset: string;
  end: string;
  sourceBytes: string;
  batch: number;
  arrivedMs: number;
  acknowledgeStartedMs?: number;
  acknowledgedMs: number;
}
/** Explicit probe build only. Browser-local clocks, no terminal text or wire retention. */
export class TerminalOutputTiming {
  private offset: bigint | undefined;
  private active: OutputBatchTiming | undefined;
  private readonly batches: OutputBatchTiming[] = [];
  private readonly frames: OutputFrameTiming[] = [];
  private readonly acknowledgements = new Map<number, number>();
  private overflow = false;
  private closed = false;
  yieldAfterFence = false;
  private unbindControl: (() => void) | undefined;
  constructor(readonly now: () => number = () => performance.now()) {}
  /** Fixed physical-key experiment in the explicit timing bundle only. */
  readonly control = (event: KeyboardEvent): void => {
    if (
      this.closed ||
      this.active ||
      !event.isTrusted ||
      event.code !== "F9" ||
      !event.ctrlKey ||
      !event.shiftKey ||
      event.altKey ||
      event.metaKey ||
      event.repeat ||
      event.isComposing
    )
      return;
    event.preventDefault();
    event.stopImmediatePropagation();
    this.yieldAfterFence = true;
  };
  bindControl(element: HTMLElement, signal: AbortSignal): void {
    element.addEventListener("keydown", this.control, true);
    const unbind = () => {
      element.removeEventListener("keydown", this.control, true);
      signal.removeEventListener("abort", unbind);
    };
    this.unbindControl = unbind;
    signal.addEventListener("abort", unbind, { once: true });
    if (signal.aborted) unbind();
  }
  ready(offset: bigint): void {
    if (!this.closed) this.offset = offset;
  }
  begin(bytes: number): OutputBatchTiming | undefined {
    if (this.closed || this.offset === undefined || this.overflow) return;
    if (this.batches.length >= 256) {
      this.overflow = true;
      return;
    }
    const offset = this.offset;
    this.offset += BigInt(bytes);
    this.active = {
      id: this.batches.length + 1,
      offset: offset.toString(),
      end: this.offset.toString(),
      startedMs: this.now(),
      parsedMs: 0,
      preparedMs: 0,
      firstFrameMs: 0,
      lastFrameMs: 0,
      frameCount: 0,
      fencedMs: 0,
      verifiedMs: 0,
      postFrameTask: this.yieldAfterFence,
    };
    return this.active;
  }
  mark(
    batch: OutputBatchTiming | undefined,
    stage: "parsedMs" | "preparedMs" | "fencedMs",
  ): void {
    if (batch && !this.closed) batch[stage] = this.now();
  }
  readonly opportunity = (): void => {
    const batch = this.active;
    if (!batch || this.closed) return;
    const now = this.now();
    if (!batch.frameCount) batch.firstFrameMs = now;
    batch.lastFrameMs = now;
    batch.frameCount++;
  };
  complete(batch: OutputBatchTiming | undefined): void {
    if (!batch || this.closed) return;
    batch.verifiedMs = this.now();
    this.batches.push(batch);
    this.active = undefined;
  }
  /** Before the existing ACK RPC; this timestamp grants no rendering credit. */
  acknowledging(sequence: number): void {
    if (this.closed || this.overflow || !this.batches.length) return;
    if (
      this.acknowledgements.size >= 256 ||
      this.acknowledgements.has(sequence)
    ) {
      this.overflow = true;
      return;
    }
    this.acknowledgements.set(sequence, this.now());
  }
  receipt(receipt: V2Receipt | null, arrivedMs: number): void {
    const acknowledgeStartedMs = receipt
      ? this.acknowledgements.get(receipt.sequence)
      : undefined;
    if (receipt) this.acknowledgements.delete(receipt.sequence);
    if (this.closed || receipt?.event.kind !== "data" || this.overflow) return;
    if (this.frames.length >= 256) {
      this.overflow = true;
      return;
    }
    const event = receipt.event;
    const end = event.offset + BigInt(event.bytes.length);
    const batch = this.batches.find(
      (b) => BigInt(b.offset) <= event.offset && BigInt(b.end) >= end,
    );
    if (!batch) {
      this.overflow = true;
      return;
    }
    this.frames.push({
      sequence: receipt.sequence,
      offset: event.offset.toString(),
      end: end.toString(),
      sourceBytes: event.sourceBytes.toString(),
      batch: batch.id,
      arrivedMs,
      ...(acknowledgeStartedMs !== undefined ? { acknowledgeStartedMs } : {}),
      acknowledgedMs: this.now(),
    });
  }
  snapshot() {
    if (this.closed || this.active)
      throw new Error("terminal.timing_unavailable");
    return {
      schemaVersion: 1,
      overflow: this.overflow,
      batches: this.batches.map((batch) => ({ ...batch })),
      frames: this.frames.map((frame) => ({ ...frame })),
    };
  }
  dispose(): void {
    this.closed = true;
    this.unbindControl?.();
    this.unbindControl = undefined;
    this.active = undefined;
    this.batches.length = this.frames.length = 0;
    this.acknowledgements.clear();
  }
}
