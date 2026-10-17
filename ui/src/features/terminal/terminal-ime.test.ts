import { expect, it, vi } from "vitest";
import { type SurfaceDriver, TerminalSurface } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

function fixture() {
  const life = new AbortController(),
    host = new GenerationViewHost(life.signal, vi.fn());
  const root = new EventTarget();
  let input: ((text: string) => void) | undefined;
  const driver = {
    element: root,
    options: { disableStdin: true },
    hasSelection: () => true,
    attachCustomKeyEventHandler: vi.fn(),
    onData: vi.fn((callback: (text: string) => void) => {
      input = callback;
      return { dispose: vi.fn() };
    }),
    focus: vi.fn(),
    dispose: vi.fn(),
  };
  const surface = new TerminalSurface(
    host.begin(1n),
    driver as unknown as SurfaceDriver,
    vi.fn(),
  );
  surface.bindSelectionCopy();
  const handler = driver.attachCustomKeyEventHandler.mock.calls[0]?.[0] as (
    event: KeyboardEvent,
  ) => boolean;
  const send = vi.fn();
  surface.enableInput(send);
  return {
    root,
    host,
    surface,
    send,
    driver,
    handler,
    input: (text: string) => input?.(text),
  };
}
const copy = {
  type: "keydown",
  key: "c",
  ctrlKey: true,
  isComposing: false,
} as KeyboardEvent;
it("selection shortcuts yield to active IME and WebKit 229 without synthesizing input", () => {
  const f = fixture();
  expect(f.handler(copy)).toBe(false);
  expect(f.handler({ ...copy, keyCode: 229 })).toBe(true);
  f.root.dispatchEvent(new Event("compositionstart"));
  expect(f.handler(copy)).toBe(true);
  f.root.dispatchEvent(new Event("compositionend"));
  expect(f.handler(copy)).toBe(false);
  expect(f.send).not.toHaveBeenCalled();
  f.surface.dispose();
  f.host.dispose();
});
it("committed CJK and combining text use the single existing onData subscription unchanged", () => {
  const f = fixture();
  f.root.dispatchEvent(new Event("compositionstart"));
  f.root.dispatchEvent(new Event("compositionend"));
  const text = "中文にほんご한글e\u0301";
  f.input(text);
  expect(f.driver.onData).toHaveBeenCalledOnce();
  expect(f.send).toHaveBeenCalledExactlyOnceWith(text);
  f.host.retire(1n);
  f.root.dispatchEvent(new Event("compositionend"));
  f.input(text);
  expect(f.send).toHaveBeenCalledOnce();
  expect(f.handler(copy)).toBe(false);
  f.surface.dispose();
  f.host.dispose();
});
