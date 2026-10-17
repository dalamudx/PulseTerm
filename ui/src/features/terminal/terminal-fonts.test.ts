import { expect, it, vi } from "vitest";
import {
  terminalFontFamily,
  waitForBundledTerminalFont,
} from "./terminal-fonts";

const bundled = {
  fontSize: 14,
  fontFamily: "JetBrains Mono",
  cursorBlink: false,
};
function fixture() {
  const load = vi.fn(async () => [{} as FontFace]);
  const check = vi.fn(() => true);
  return {
    load,
    check,
    doc: { fonts: { load, check } } as unknown as Document,
    life: new AbortController(),
  };
}
it("only the local bundled family is awaited and given a generic fallback", async () => {
  const f = fixture();
  await waitForBundledTerminalFont(
    f.doc,
    { ...bundled, fontFamily: "monospace" },
    f.life.signal,
  );
  expect(f.load).not.toHaveBeenCalled();
  await waitForBundledTerminalFont(f.doc, bundled, f.life.signal);
  expect(f.load).toHaveBeenCalledWith('14px "JetBrains Mono"', "M");
  expect(terminalFontFamily(bundled)).toBe('"JetBrains Mono", monospace');
});
it("an unregistered or failing bundled face is rejected before measurement", async () => {
  const f = fixture();
  f.load.mockResolvedValue([]);
  await expect(
    waitForBundledTerminalFont(f.doc, bundled, f.life.signal),
  ).rejects.toThrow("terminal.font_unavailable");
  f.load.mockRejectedValue(Error("external failure"));
  await expect(
    waitForBundledTerminalFont(f.doc, bundled, f.life.signal),
  ).rejects.toThrow("terminal.font_unavailable");
});
it("page cancellation wins over a later font response", async () => {
  const f = fixture();
  let finish!: (faces: FontFace[]) => void;
  f.load.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const pending = waitForBundledTerminalFont(f.doc, bundled, f.life.signal);
  const rejected = expect(pending).rejects.toThrow("view.retired");
  await Promise.resolve();
  f.life.abort();
  await rejected;
  finish([{} as FontFace]);
  await Promise.resolve();
  expect(f.check).not.toHaveBeenCalled();
});
it("a stalled face has a bounded deadline", async () => {
  vi.useFakeTimers();
  try {
    const f = fixture();
    f.load.mockImplementation(() => new Promise(() => {}));
    const rejected = expect(
      waitForBundledTerminalFont(f.doc, bundled, f.life.signal),
    ).rejects.toThrow("terminal.font_timeout");
    await vi.advanceTimersByTimeAsync(1500);
    await rejected;
  } finally {
    vi.useRealTimers();
  }
});
