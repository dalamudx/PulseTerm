// Reconnects are transactions on ONE ordered stream; never JSON-wrap VT bytes.
import { decodeFrame } from "./frames";
export const ATTACHMENT_KIND = 8;
export const ENVELOPE_LENGTH = 20;
export const MAX_FRAGMENT = 64 * 1024;
export const MAX_HISTORY = 4 * 1024 * 1024;
export const MAX_RESTORE = 1024 * 1024;
const MAX_U64 = (1n << 64n) - 1n;
export interface AttachBoundary {
  columns: number;
  rows: number;
  revision: bigint;
  parsedBytes: bigint;
}
export type AttachmentEvent =
  | { kind: "staged" }
  | {
      kind: "ready";
      boundary: AttachBoundary;
      history: Uint8Array;
      restore: Uint8Array;
    }
  | { kind: "data"; bytes: Uint8Array; offset: bigint };
/** ACK only after consuming/copying staging bytes, or completing ready/data
 * rendering. Stale generations return null and do not receive a current ACK. */
export interface AttachmentReceipt {
  generation: bigint;
  sequence: number;
  event: AttachmentEvent;
}
/** Bounded transaction receiver. Malformed current-generation data poisons this
 * receiver: cancel and start a NEW generation; no rollback/partial render. */
export class AttachmentReceiver {
  private next = 0;
  private phase: "begin" | "bootstrap" | "live" | "failed" = "begin";
  private boundary: AttachBoundary | undefined;
  private history = new Uint8Array();
  private restore = new Uint8Array();
  private historyAt = 0;
  private restoreAt = 0;
  private liveAt = 0n;
  constructor(readonly generation: bigint) {
    if (generation <= 0n || generation > MAX_U64)
      throw new Error("attach.generation");
  }
  accept(bytes: Uint8Array): AttachmentReceipt | null {
    if (this.phase === "failed") throw new Error("attach.failed");
    try {
      return this.consume(bytes);
    } catch (error) {
      this.phase = "failed";
      this.history = new Uint8Array();
      this.restore = new Uint8Array();
      this.boundary = undefined;
      throw error;
    }
  }
  private consume(bytes: Uint8Array): AttachmentReceipt | null {
    const frame = decodeFrame(bytes);
    if (frame.kind !== ATTACHMENT_KIND) throw new Error("attach.kind");
    const payload = frame.payload;
    if (
      payload.length < ENVELOPE_LENGTH ||
      payload.length > ENVELOPE_LENGTH + MAX_FRAGMENT
    )
      throw new Error("attach.length");
    const view = new DataView(
      payload.buffer,
      payload.byteOffset,
      payload.byteLength,
    );
    if (view.getUint8(0) !== 1 || view.getUint16(2, true) !== 0)
      throw new Error("attach.version_or_flags");
    const generation = view.getBigUint64(4, true);
    if (generation === 0n) throw new Error("attach.generation");
    if (generation !== this.generation) return null;
    if (frame.sequence !== this.next) throw new Error("attach.sequence");
    const part = view.getUint8(1);
    const offset = view.getBigUint64(12, true);
    const body = payload.subarray(ENVELOPE_LENGTH);
    let event: AttachmentEvent = { kind: "staged" };
    if (part === 1) {
      if (this.phase !== "begin" || offset !== 0n || body.length !== 28)
        throw new Error("attach.begin");
      const meta = new DataView(body.buffer, body.byteOffset, body.byteLength);
      const boundary = {
        columns: meta.getUint16(0, true),
        rows: meta.getUint16(2, true),
        revision: meta.getBigUint64(4, true),
        parsedBytes: meta.getBigUint64(12, true),
      };
      const historyBytes = meta.getUint32(20, true);
      const restoreBytes = meta.getUint32(24, true);
      if (
        boundary.columns < 1 ||
        boundary.columns > 512 ||
        boundary.rows < 1 ||
        boundary.rows > 256 ||
        historyBytes > MAX_HISTORY ||
        restoreBytes < 1 ||
        restoreBytes > MAX_RESTORE
      )
        throw new Error("attach.limits");
      this.boundary = boundary;
      this.history = new Uint8Array(historyBytes);
      this.restore = new Uint8Array(restoreBytes);
      this.liveAt = boundary.parsedBytes;
      this.phase = "bootstrap";
    } else if (part === 2 || part === 3) {
      if (this.phase !== "bootstrap" || body.length === 0)
        throw new Error("attach.phase");
      if (part === 2) {
        if (
          this.restoreAt !== 0 ||
          offset !== BigInt(this.historyAt) ||
          body.length > this.history.length - this.historyAt
        )
          throw new Error("attach.history");
        this.history.set(body, this.historyAt);
        this.historyAt += body.length;
      } else {
        if (
          this.historyAt !== this.history.length ||
          offset !== BigInt(this.restoreAt) ||
          body.length > this.restore.length - this.restoreAt
        )
          throw new Error("attach.restore");
        this.restore.set(body, this.restoreAt);
        this.restoreAt += body.length;
      }
    } else if (part === 4) {
      if (
        this.phase !== "bootstrap" ||
        offset !== 0n ||
        body.length !== 0 ||
        this.historyAt !== this.history.length ||
        this.restoreAt !== this.restore.length ||
        this.boundary === undefined
      )
        throw new Error("attach.commit");
      event = {
        kind: "ready",
        boundary: this.boundary,
        history: this.history,
        restore: this.restore,
      };
      this.history = new Uint8Array();
      this.restore = new Uint8Array();
      this.boundary = undefined;
      this.phase = "live";
    } else if (part === 5) {
      if (
        this.phase !== "live" ||
        body.length === 0 ||
        offset !== this.liveAt ||
        this.liveAt + BigInt(body.length) > MAX_U64
      )
        throw new Error("attach.data");
      event = { kind: "data", bytes: body, offset };
      this.liveAt += BigInt(body.length);
    } else {
      throw new Error("attach.part");
    }
    this.next = (this.next + 1) >>> 0;
    return { generation, sequence: frame.sequence, event };
  }
}
