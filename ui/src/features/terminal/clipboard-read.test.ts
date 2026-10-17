import { expect, it, vi } from "vitest";
import { readSystemClipboard } from "./clipboard-read";

function fixture() {
  const read = vi.fn(async () => "fixture"),
    abort = new AbortController();
  const doc = {
    defaultView: { navigator: { clipboard: { readText: read } } },
  } as unknown as Document;
  return { read, abort, doc };
}
it("starts the browser read synchronously in its original event", async () => {
  const f = fixture();
  const pending = readSystemClipboard(f.doc, f.abort.signal, () => true);
  expect(f.read).toHaveBeenCalledOnce();
  expect(await pending).toBe("fixture");
});
it("does no browser read after cancellation or view retirement", async () => {
  const f = fixture();
  f.abort.abort();
  await expect(
    readSystemClipboard(f.doc, f.abort.signal, () => true),
  ).rejects.toThrow("paste.cancelled");
  const other = fixture();
  await expect(
    readSystemClipboard(other.doc, other.abort.signal, () => false),
  ).rejects.toThrow("paste.cancelled");
  expect(f.read).not.toHaveBeenCalled();
  expect(other.read).not.toHaveBeenCalled();
});
it("cancellation waits for the actual read to exit and discards its late text", async () => {
  const f = fixture();
  let finish!: (text: string) => void;
  f.read.mockImplementation(
    () =>
      new Promise<string>((resolve) => {
        finish = resolve;
      }),
  );
  let finished = false;
  const pending = readSystemClipboard(
    f.doc,
    f.abort.signal,
    () => true,
  ).finally(() => {
    finished = true;
  });
  const rejected = expect(pending).rejects.toThrow("paste.cancelled");
  f.abort.abort();
  await Promise.resolve();
  expect(finished).toBe(false);
  finish("retired");
  await rejected;
});
it("rejects a view retired during the read and does not retry failures", async () => {
  const f = fixture();
  let current = true;
  f.read.mockImplementation(async () => {
    current = false;
    return "stale";
  });
  await expect(
    readSystemClipboard(f.doc, f.abort.signal, () => current),
  ).rejects.toThrow("paste.cancelled");
  const other = fixture();
  other.read.mockRejectedValue(Error("denied"));
  await expect(
    readSystemClipboard(other.doc, other.abort.signal, () => true),
  ).rejects.toThrow("denied");
  expect(other.read).toHaveBeenCalledOnce();
});
it("unsupported browser rejects without exposing a native permission route", async () => {
  const f = fixture();
  const doc = { defaultView: { navigator: {} } } as Document;
  await expect(
    readSystemClipboard(doc, f.abort.signal, () => true),
  ).rejects.toThrow("paste.clipboard_api_missing");
});
