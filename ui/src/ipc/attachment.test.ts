import { readFileSync } from "node:fs";
import { expect, it } from "vitest";
import {
  type AttachmentReceipt,
  AttachmentReceiver,
  MAX_HISTORY,
} from "./attachment";

const epoch = (1n << 60n) + 7n;
const fixture = readFileSync(
  new URL("../../../tests/fixtures/frames/attachment.hex", import.meta.url),
  "utf8",
)
  .trim()
  .split("\n")
  .map((line) =>
    Uint8Array.from(line.trim().split(/\s+/), (v) => Number.parseInt(v, 16)),
  );
const copy = (i: number) => {
  const f = fixture[i];
  if (!f) throw new Error("fixture");
  return f.slice();
};
function feed(
  receiver: AttachmentReceiver,
  frames = fixture,
): AttachmentReceipt[] {
  return frames.flatMap((frame) => {
    const receipt = receiver.accept(frame);
    return receipt ? [receipt] : [];
  });
}
it("uses the same Rust encoder fixture and preserves u64 precision", () => {
  const receipts = feed(new AttachmentReceiver(epoch));
  expect(receipts.slice(0, -2).every((r) => r.event.kind === "staged")).toBe(
    true,
  );
  const ready = receipts.find((r) => r.event.kind === "ready")?.event;
  if (ready?.kind !== "ready") throw new Error("missing commit");
  expect(ready.boundary).toEqual({
    columns: 32,
    rows: 8,
    revision: (1n << 64n) - 5n,
    parsedBytes: 9n,
  });
  expect(new TextDecoder().decode(ready.history)).toBe("older\r\n");
  expect(new TextDecoder().decode(ready.restore)).toBe("SCREEN");
  const last = receipts.at(-1);
  expect(last?.generation).toBe(epoch);
  expect(last?.event).toMatchObject({
    kind: "data",
    offset: 9n,
    bytes: new Uint8Array([90]),
  });
});
it("ignores stale generations without advancing sequence or sending current credit", () => {
  const receiver = new AttachmentReceiver(epoch);
  const old = copy(0);
  new DataView(old.buffer).setBigUint64(10, epoch - 1n, true);
  expect(receiver.accept(old)).toBeNull();
  expect(feed(receiver)).toHaveLength(8);
});
it("supports byte views with an offset", () => {
  const receiver = new AttachmentReceiver(epoch);
  const receipts = feed(
    receiver,
    fixture.map((frame) => {
      const buffer = new Uint8Array(frame.length + 7);
      buffer.set(frame, 3);
      return buffer.subarray(3, 3 + frame.length);
    }),
  );
  expect(receipts.at(-1)?.event.kind).toBe("data");
});
it("rejects declared sizes before allocation and poisons the receiver", () => {
  const receiver = new AttachmentReceiver(epoch);
  const begin = copy(0);
  new DataView(begin.buffer).setUint32(6 + 20 + 20, MAX_HISTORY + 1, true);
  expect(() => receiver.accept(begin)).toThrow("attach.limits");
  expect(() => receiver.accept(copy(0))).toThrow("attach.failed");
});
it("does not expose partial restore on cancellation or truncated commit", () => {
  const receiver = new AttachmentReceiver(epoch);
  const receipts = feed(receiver, fixture.slice(0, 6));
  expect(receipts.every((r) => r.event.kind === "staged")).toBe(true);
  const commit = copy(6);
  new DataView(commit.buffer).setUint32(2, 5, true);
  expect(() => receiver.accept(commit)).toThrow("attach.sequence");
});
it("rejects duplicated, skipped or reordered fragments", () => {
  for (const i of [0, 2, 4, 6, 7]) {
    const r = new AttachmentReceiver(epoch);
    r.accept(copy(0));
    expect(() => r.accept(copy(i))).toThrow();
    expect(() => r.accept(copy(1))).toThrow("attach.failed");
  }
});
it("rejects wrong offsets and live output before Commit", () => {
  const r = new AttachmentReceiver(epoch);
  r.accept(copy(0));
  const wrong = copy(1);
  new DataView(wrong.buffer).setBigUint64(18, 1n, true);
  expect(() => r.accept(wrong)).toThrow("attach.history");
  const premature = new AttachmentReceiver(epoch);
  premature.accept(copy(0));
  const live = copy(7);
  new DataView(live.buffer).setUint32(2, 1, true);
  expect(() => premature.accept(live)).toThrow("attach.data");
  const committed = new AttachmentReceiver(epoch);
  feed(committed, fixture.slice(0, 7));
  const gap = copy(7);
  new DataView(gap.buffer).setBigUint64(18, 10n, true);
  expect(() => committed.accept(gap)).toThrow("attach.data");
});
it("requires exact totals, version, flags, kind and nonzero epoch", () => {
  const mutations = [
    (f: Uint8Array) => {
      f[6] = 2;
    },
    (f: Uint8Array) => {
      f[8] = 1;
    },
    (f: Uint8Array) => {
      f[0] = 1;
    },
    (f: Uint8Array) => {
      f[7] = 99;
    },
    (f: Uint8Array) => new DataView(f.buffer).setBigUint64(10, 0n, true),
  ];
  for (const mutate of mutations) {
    const r = new AttachmentReceiver(epoch);
    const f = copy(0);
    mutate(f);
    expect(() => r.accept(f)).toThrow();
  }
  const r = new AttachmentReceiver(epoch);
  r.accept(copy(0));
  const commit = copy(6);
  new DataView(commit.buffer).setUint32(2, 1, true);
  expect(() => r.accept(commit)).toThrow("attach.commit");
});
