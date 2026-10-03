import { decodeFrame } from "../../ipc/frames";

import type { ProbeConfig } from "../../ipc/gen/commands";

export type { ProbeConfig } from "../../ipc/gen/commands";

/** Synthetic full-byte integrity check. Does not render terminal or desktop pixels. */
export class FrameVerifier {
  frames = 0;
  readonly deliveryMs: number[] = [];
  constructor(
    readonly config: ProbeConfig,
    readonly clockOffsetMs: number,
  ) {}

  consume(bytes: Uint8Array, receivedMs: number): number {
    const frame = decodeFrame(bytes);
    if (
      frame.kind !== 1 ||
      frame.sequence !== this.frames ||
      this.frames >= this.config.frames
    )
      throw new Error("probe.sequence");
    if (frame.payload.byteLength !== this.config.chunkBytes)
      throw new Error("probe.length");
    const view = new DataView(
      frame.payload.buffer,
      frame.payload.byteOffset,
      frame.payload.byteLength,
    );
    const sentMs = view.getFloat64(0, true);
    if (!Number.isFinite(sentMs) || sentMs < 0)
      throw new Error("probe.timestamp");
    for (let index = 8; index < frame.payload.length; index++) {
      if (frame.payload[index] !== ((frame.sequence + index) & 255))
        throw new Error("probe.corrupt");
    }
    this.deliveryMs.push(receivedMs - sentMs - this.clockOffsetMs);
    this.frames++;
    return frame.sequence;
  }

  finish(): void {
    if (this.frames !== this.config.frames) throw new Error("probe.incomplete");
  }
}

export function percentile(values: readonly number[], percent: number): number {
  if (values.length === 0) return 0;
  const sorted = [...values].sort((a, b) => a - b);
  return (
    sorted[Math.max(0, Math.ceil((sorted.length * percent) / 100) - 1)] ?? 0
  );
}
