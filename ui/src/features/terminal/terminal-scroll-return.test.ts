import { beforeEach, expect, it, vi } from "vitest";
import { createTerminalSurface, type SurfaceDriver } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

const notice = vi.hoisted(() => ({ refresh: vi.fn(), dispose: vi.fn() }));
vi.mock("./terminal-scroll-return", () => ({
  createScrollReturn: () => notice,
}));
beforeEach(() => vi.clearAllMocks());
async function fixture() {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n),
    pending: (() => void)[] = [];
  let scroll = () => {};
  const stop = vi.fn(),
    hook = () => ({ dispose: vi.fn() });
  const terminal = {
    options: { disableStdin: true },
    parser: {
      registerCsiHandler: hook,
      registerDcsHandler: hook,
      registerEscHandler: hook,
      registerOscHandler: hook,
    },
    unicode: { versions: ["6"], activeVersion: "6", register: vi.fn() },
    attachCustomKeyEventHandler: vi.fn(),
    open: vi.fn(),
    dispose: vi.fn(),
    onData: vi.fn(),
    focus: vi.fn(),
    onScroll: vi.fn((listener: () => void) => {
      scroll = listener;
      return { dispose: stop };
    }),
    scrollToBottom: vi.fn(),
    buffer: { active: { viewportY: 0, baseY: 0 } },
    write: vi.fn((_text: string, done: () => void) => pending.push(done)),
  };
  const document = {
    querySelector: () => ({ nonce: "runtime-style-nonce" }),
    createElement: () => ({ setAttribute: vi.fn() }),
  } as unknown as Document;
  const palette = JSON.stringify({
    version: 1,
    palette: {
      foreground: [200, 200, 200],
      background: [0, 0, 0],
      cursor: [255, 255, 255],
      ansi: Array.from({ length: 16 }, () => [20, 30, 40]),
    },
  });
  const surface = await createTerminalSurface(
    scope,
    { ownerDocument: document } as HTMLElement,
    palette,
    { columns: 32, rows: 8, history: 16 },
    async () => () => terminal as unknown as SurfaceDriver,
  );
  return { host, surface, terminal, pending, stop, scroll: () => scroll() };
}
it("updates on user scrolling and parsed output without enabling input or auto-scrolling", async () => {
  const f = await fixture();
  try {
    expect(notice.refresh).toHaveBeenCalledOnce();
    f.scroll();
    expect(notice.refresh).toHaveBeenCalledTimes(2);
    const write = f.surface.writeParsed(new TextEncoder().encode("new output"));
    expect(notice.refresh).toHaveBeenCalledTimes(2);
    f.pending.shift()?.();
    await write;
    expect(notice.refresh).toHaveBeenCalledTimes(3);
    expect(f.terminal.scrollToBottom).not.toHaveBeenCalled();
    expect(f.terminal.options.disableStdin).toBe(true);
    expect(f.terminal.onData).not.toHaveBeenCalled();
  } finally {
    f.host.dispose();
  }
});
it("ignores late scroll and parsed output after retirement", async () => {
  const f = await fixture();
  const write = f.surface.writeParsed(new TextEncoder().encode("pending"));
  const rejected = expect(write).rejects.toThrow("view.retired");
  f.host.dispose();
  f.scroll();
  f.pending.shift()?.();
  await rejected;
  expect(notice.refresh).toHaveBeenCalledOnce();
  expect(notice.dispose).toHaveBeenCalledOnce();
  expect(f.stop).toHaveBeenCalledOnce();
});
it("releases its listener and UI only once on repeated disposal", async () => {
  const f = await fixture();
  f.surface.dispose();
  f.surface.dispose();
  f.scroll();
  f.host.dispose();
  expect(notice.dispose).toHaveBeenCalledOnce();
  expect(f.stop).toHaveBeenCalledOnce();
  expect(notice.refresh).toHaveBeenCalledOnce();
});
