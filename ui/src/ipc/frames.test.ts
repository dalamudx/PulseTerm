import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { decodeFrame, HEADER_LENGTH, MAX_PAYLOAD } from "./frames";

describe("shared binary frame codec", () => {
  it("reads the exact same fixture as Rust", () => {
    const hex = readFileSync(
      new URL("../../../tests/fixtures/frames/data.hex", import.meta.url),
      "utf8",
    );
    const bytes = Uint8Array.from(hex.trim().split(/\s+/), (v) =>
      Number.parseInt(v, 16),
    );
    const frame = decodeFrame(bytes);
    expect(frame.kind).toBe(1);
    expect(frame.sequence).toBe(0x12345678);
    expect(new TextDecoder().decode(frame.payload)).toBe("hello\r\n");
    expect(frame.payload.buffer).toBe(bytes.buffer);
  });

  it("handles a view with a nonzero buffer offset", () => {
    const bytes = new Uint8Array([99, 99, 1, 0, 255, 255, 255, 255, 42, 99]);
    expect(decodeFrame(bytes.subarray(2, 9))).toEqual({
      kind: 1,
      sequence: 0xffffffff,
      payload: new Uint8Array([42]),
    });
  });

  it("rejects malformed messages before using their payload", () => {
    for (let length = 0; length < HEADER_LENGTH; length++) {
      expect(() => decodeFrame(new Uint8Array(length))).toThrow(
        "ipc.truncated",
      );
    }
    expect(() => decodeFrame(new Uint8Array([255, 0, 0, 0, 0, 0]))).toThrow(
      "ipc.unknown_kind",
    );
    expect(() => decodeFrame(new Uint8Array([1, 1, 0, 0, 0, 0]))).toThrow(
      "ipc.unsupported_flags",
    );
    expect(() =>
      decodeFrame(new Uint8Array(MAX_PAYLOAD + HEADER_LENGTH + 1)),
    ).toThrow("ipc.too_large");
  });
});
