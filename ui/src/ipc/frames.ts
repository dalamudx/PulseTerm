/** Binary messages are session-local. The enclosing Channel supplies boundaries. */
export const HEADER_LENGTH = 6;
export const MAX_PAYLOAD = 1024 * 1024;

const kinds = new Set([
  1, 2, 3, 4, 5, 6, 7, 0x20, 0x21, 0x22, 0x23, 0x24, 0x40, 0x41, 0x60,
]);

export interface Frame {
  kind: number;
  sequence: number;
  payload: Uint8Array;
}

export function decodeFrame(bytes: Uint8Array): Frame {
  if (bytes.byteLength < HEADER_LENGTH) throw new Error("ipc.truncated");
  if (bytes.byteLength - HEADER_LENGTH > MAX_PAYLOAD)
    throw new Error("ipc.too_large");
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  const kind = view.getUint8(0);
  if (!kinds.has(kind)) throw new Error("ipc.unknown_kind");
  if (view.getUint8(1) !== 0) throw new Error("ipc.unsupported_flags");
  return {
    kind,
    sequence: view.getUint32(2, true),
    payload: bytes.subarray(HEADER_LENGTH),
  };
}
