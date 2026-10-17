import { beforeEach, expect, it, vi } from "vitest";
import { createTerminalSurface, type SurfaceDriver } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

const notice = vi.hoisted(() => ({ ring: vi.fn(), dispose: vi.fn() }));
vi.mock("./terminal-bell", () => ({ createTerminalBellNotice: () => notice }));
beforeEach(() => vi.clearAllMocks());

async function fixture() {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n);
  let emit = () => {};
  const pending: (() => void)[] = [];
  const hook = () => ({ dispose: vi.fn() });
  const stop = vi.fn();
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
    onBell: vi.fn((listener: () => void) => {
      emit = listener;
      return { dispose: stop };
    }),
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
  return { host, scope, surface, terminal, pending, stop, emit: () => emit() };
}
const bytes = new TextEncoder().encode("\u0007");

it("suppresses replay and unsolicited bell events without granting input", async () => {
  const f = await fixture();
  try {
    f.emit();
    const write = f.surface.writeParsed(bytes);
    f.emit();
    f.pending.shift()?.();
    await write;
    expect(notice.ring).not.toHaveBeenCalled();
    expect(f.terminal.options.disableStdin).toBe(true);
    expect(f.terminal.onData).not.toHaveBeenCalled();
  } finally {
    f.host.dispose();
  }
});
it("allows bells only while current live bytes are being parsed", async () => {
  const f = await fixture();
  try {
    const write = f.surface.writeParsed(bytes, new AbortController().signal);
    f.emit();
    expect(notice.ring).toHaveBeenCalledOnce();
    f.pending.shift()?.();
    await write;
    f.emit();
    expect(notice.ring).toHaveBeenCalledOnce();
    expect(f.terminal.options.disableStdin).toBe(true);
  } finally {
    f.host.dispose();
  }
});
it("ignores bells when the individual frame is cancelled during parsing", async () => {
  const f = await fixture(),
    frame = new AbortController();
  try {
    const write = f.surface.writeParsed(bytes, frame.signal);
    frame.abort();
    f.emit();
    f.pending.shift()?.();
    await write;
    expect(notice.ring).not.toHaveBeenCalled();
    expect(f.scope.isCurrent()).toBe(true);
  } finally {
    f.host.dispose();
  }
});
it("rejects late bells and releases the listener and notice when the view retires", async () => {
  const f = await fixture();
  const write = f.surface.writeParsed(bytes, new AbortController().signal);
  const rejected = expect(write).rejects.toThrow("view.retired");
  f.host.dispose();
  f.emit();
  f.pending.shift()?.();
  await rejected;
  expect(notice.ring).not.toHaveBeenCalled();
  expect(f.stop).toHaveBeenCalledOnce();
  expect(notice.dispose).toHaveBeenCalledOnce();
  f.surface.dispose();
  expect(f.stop).toHaveBeenCalledOnce();
});
