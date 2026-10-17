import { expect, it, vi } from "vitest";
import { assessPaste, MAX_PASTE_BYTES, PasteGate } from "./paste-policy";

it("bounds UTF8 including bracketed framing and rejects replacement/hidden executable controls", () => {
  expect(assessPaste("x".repeat(MAX_PASTE_BYTES)).bytes).toBe(MAX_PASTE_BYTES);
  for (const text of [
    "",
    "x".repeat(MAX_PASTE_BYTES + 1),
    "中".repeat(400),
    "a\ud800",
    "x\x1b[201~",
    "x\x00",
    "\x03",
    "\u009b31m",
  ])
    expect(() => assessPaste(text)).toThrow();
});
it("newline variants, tab completion and directional controls require confirmation", () => {
  for (const text of ["a\nb", "a\r\nb", "a\rb", "a\n", "a\tb", "a\u202eb"])
    expect(assessPaste(text).confirmation).toBe(true);
  expect(assessPaste("中文🙂 hello").confirmation).toBe(false);
  for (const text of [
    "\ufeffabc",
    "a\u061cb",
    "a\u2060b",
    "a\u2028b",
    "a\u2029b",
  ])
    expect(assessPaste(text).confirmation).toBe(true);
  expect(assessPaste("a\r\nb").lines).toBe(2);
});
it("single line submits once, no confirmation or automatic resend", async () => {
  const send = vi.fn(),
    confirm = vi.fn(async () => true);
  const gate = new PasteGate(
    new AbortController().signal,
    () => true,
    confirm,
    send,
  );
  expect(await gate.submit("hello")).toBe("submitted");
  expect(confirm).not.toHaveBeenCalled();
  expect(send).toHaveBeenCalledExactlyOnceWith("hello");
  gate.close();
  expect(await gate.submit("hello")).toBe("cancelled");
  expect(send).toHaveBeenCalledTimes(1);
});
it("cancel or failed modal sends nothing", async () => {
  const send = vi.fn();
  for (const confirm of [
    async () => false,
    async () => {
      throw Error("dialog unavailable");
    },
  ]) {
    const gate = new PasteGate(
      new AbortController().signal,
      () => true,
      confirm,
      send,
    );
    expect(await gate.submit("a\nb")).toBe("cancelled");
    gate.close();
  }
  expect(send).not.toHaveBeenCalled();
});
it("late confirmation after page/resize/EOF close never dispatches", async () => {
  const controller = new AbortController(),
    send = vi.fn();
  let accept!: (v: boolean) => void;
  let decision: AbortSignal | undefined;
  const gate = new PasteGate(
    controller.signal,
    () => true,
    (_t, _a, signal) => {
      decision = signal;
      return new Promise((resolve) => {
        accept = resolve;
      });
    },
    send,
  );
  const job = gate.submit("a\nb");
  controller.abort();
  expect(decision?.aborted).toBe(true);
  accept(true);
  expect(await job).toBe("cancelled");
  expect(send).not.toHaveBeenCalled();
});
it("one pending decision refuses another candidate", async () => {
  let accept!: (v: boolean) => void;
  const confirm = vi.fn(
      () =>
        new Promise<boolean>((resolve) => {
          accept = resolve;
        }),
    ),
    send = vi.fn(),
    gate = new PasteGate(
      new AbortController().signal,
      () => true,
      confirm,
      send,
    );
  const first = gate.submit("a\nb");
  expect(await gate.submit("different\ncommand")).toBe("busy");
  expect(confirm).toHaveBeenCalledTimes(1);
  accept(true);
  expect(await first).toBe("submitted");
  expect(send).toHaveBeenCalledExactlyOnceWith("a\nb");
  gate.close();
});
it("lost view and uncertain send are never success/retry", async () => {
  let current = true;
  let accept!: (v: boolean) => void;
  const send = vi.fn(),
    gate = new PasteGate(
      new AbortController().signal,
      () => current,
      () =>
        new Promise((resolve) => {
          accept = resolve;
        }),
      send,
    );
  const job = gate.submit("a\nb");
  current = false;
  accept(true);
  expect(await job).toBe("cancelled");
  expect(send).not.toHaveBeenCalled();
  gate.close();
  const uncertain = vi.fn(() => {
    throw Error("uncertain");
  });
  const next = new PasteGate(
    new AbortController().signal,
    () => true,
    async () => true,
    uncertain,
  );
  await expect(next.submit("one")).rejects.toThrow("uncertain");
  expect(uncertain).toHaveBeenCalledTimes(1);
  next.close();
});
