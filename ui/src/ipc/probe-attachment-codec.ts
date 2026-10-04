// Fixed UTF-8 probe adapter only; general v2 consumers use both coordinates directly.

import {
  type AttachBoundary,
  type AttachmentReceipt,
  attachmentEnvelope,
} from "./attachment";
import { AttachmentConsumer, type AttachmentSink } from "./attachment-consumer";
import {
  AttachmentV2Consumer,
  V2_ENVELOPE_LENGTH,
  type V2Boundary,
  type V2Receipt,
} from "./attachment-v2";
export type ProbeAttachmentVersion = 1 | 2;
function projected(b: V2Boundary): AttachBoundary {
  if (b.sourceBytes !== b.utf8Bytes)
    throw new Error("session.probe_requires_utf8_boundary");
  return {
    columns: b.columns,
    rows: b.rows,
    revision: b.revision,
    parsedBytes: b.sourceBytes,
  };
}
function receipt(r: V2Receipt | null): AttachmentReceipt | null {
  if (!r) return null;
  const e = r.event;
  switch (e.kind) {
    case "ready":
    case "repair":
      return { ...r, event: { ...e, boundary: projected(e.boundary) } };
    case "end":
    case "marker":
    case "checkpoint":
      if (e.sourceBytes !== e.utf8Bytes)
        throw new Error("session.probe_requires_utf8_boundary");
      return {
        ...r,
        event: {
          kind: e.kind,
          revision: e.revision,
          parsedBytes: e.sourceBytes,
        },
      };
    default:
      return { ...r, event: e };
  }
}
export interface ProbeConsumer {
  enqueue(wire: Uint8Array): Promise<AttachmentReceipt | null>;
  cancel(error?: Error): void;
}
export function probeAttachmentEnvelope(
  payload: Uint8Array,
  version: ProbeAttachmentVersion,
) {
  if (version === 1) return attachmentEnvelope(payload);
  if (
    payload.length < V2_ENVELOPE_LENGTH ||
    payload.length > V2_ENVELOPE_LENGTH + 65536
  )
    throw new Error("attach2.length");
  const view = new DataView(
    payload.buffer,
    payload.byteOffset,
    payload.byteLength,
  );
  if (payload[0] !== 2 || view.getUint16(2, true) !== 0)
    throw new Error("attach2.version_or_flags");
  const generation = view.getBigUint64(4, true);
  if (generation === 0n) throw new Error("attach2.generation");
  return {
    generation,
    part: view.getUint8(1),
    offset: view.getBigUint64(12, true),
  };
}
export function createProbeConsumer(
  version: ProbeAttachmentVersion,
  generation: bigint,
  sink: AttachmentSink,
  ack: (generation: bigint, sequence: number) => void | Promise<void>,
  maxFrames = 8,
  maxBytes = 8 * (6 + (version === 2 ? 28 : 20) + 65536),
): ProbeConsumer {
  if (version === 1)
    return new AttachmentConsumer(generation, sink, ack, maxFrames, maxBytes);
  const consumer = new AttachmentV2Consumer(
    generation,
    {
      reset: (b, s) => sink.reset(projected(b), s),
      write: (bytes, s) => sink.write(bytes, s),
      ...(sink.repair
        ? {
            repair: (b: V2Boundary, bytes: Uint8Array, s: AbortSignal) =>
              sink.repair?.(projected(b), bytes, s) ??
              Promise.reject(new Error("attach2.repair_unsupported")),
          }
        : {}),
      ...(sink.marker
        ? {
            marker: async (b, s) => {
              if (b.sourceBytes !== b.utf8Bytes)
                throw new Error("session.probe_requires_utf8_boundary");
              await sink.marker?.(b.revision, b.sourceBytes, s);
            },
          }
        : {}),
      ...(sink.checkpoint
        ? {
            checkpoint: async (b, s) => {
              if (b.sourceBytes !== b.utf8Bytes)
                throw new Error("session.probe_requires_utf8_boundary");
              await sink.checkpoint?.(b.revision, b.sourceBytes, s);
            },
          }
        : {}),
      ...(sink.finish
        ? {
            finish: async (b, s) => {
              if (b.sourceBytes !== b.utf8Bytes)
                throw new Error("session.probe_requires_utf8_boundary");
              await sink.finish?.(b.revision, b.sourceBytes, s);
            },
          }
        : {}),
    },
    ack,
    maxFrames,
    maxBytes,
  );
  return {
    enqueue: async (wire) => receipt(await consumer.enqueue(wire)),
    cancel: (e) => consumer.cancel(e),
  };
}
