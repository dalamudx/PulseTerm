import { expect, it, vi } from "vitest";
import {
  createTerminalSurface,
  type SurfaceDriver,
  type SurfaceFactory,
} from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

const palette = JSON.stringify({
  version: 1,
  palette: {
    ansi: Array.from({ length: 16 }, () => [20, 30, 40]),
    foreground: [200, 200, 200],
    background: [0, 0, 0],
    cursor: [255, 255, 255],
  },
});
const geometry = { columns: 32, rows: 8, history: 16 };
it("display focus never grants input and retired scope cannot focus", async () => {
  const f = fixture(),
    surface = await f.create();
  expect(surface.focusOutput()).toBe(true);
  expect(f.terminal.options.disableStdin).toBe(true);
  expect(f.terminal.onData).not.toHaveBeenCalled();
  f.host.begin(2n);
  expect(surface.focusOutput()).toBe(false);
  expect(f.terminal.focus).toHaveBeenCalledOnce();
  surface.dispose();
  expect(surface.focusOutput()).toBe(false);
});
function fixture(nonce = "runtime-style-nonce") {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n);
  const style = { setAttribute: vi.fn() };
  const doc = {
    querySelector: () => ({ nonce }),
    createElement: vi.fn(() => style),
  } as unknown as Document;
  const element = { ownerDocument: doc } as HTMLElement;
  const hooks: ReturnType<typeof vi.fn>[] = [];
  const osc = new Map<number, (text: string) => boolean>();
  let input: ((text: string) => void) | undefined;
  const pending: { text: string; callback: () => void }[] = [];
  const registration = () => {
    const dispose = vi.fn();
    hooks.push(dispose);
    return { dispose };
  };
  const inputDispose = vi.fn();
  const terminal = {
    parser: {
      registerCsiHandler: registration,
      registerDcsHandler: registration,
      registerOscHandler: (id: number, fn: (text: string) => boolean) => {
        osc.set(id, fn);
        return registration();
      },
    },
    unicode: { register: vi.fn(), activeVersion: "" },
    element: new EventTarget(),
    options: { disableStdin: true },
    focus: vi.fn(),
    paste: vi.fn((text: string) => input?.(text)),
    hasSelection: vi.fn(() => false),
    getSelection: vi.fn(() => ""),
    attachCustomKeyEventHandler: vi.fn(),
    open: vi.fn(),
    dispose: vi.fn(),
    write: vi.fn((text: string, callback: () => void) =>
      pending.push({ text, callback }),
    ),
    onData: vi.fn((fn: (text: string) => void) => {
      input = fn;
      return { dispose: inputDispose };
    }),
  };
  const factory: SurfaceFactory = vi.fn(
    () => terminal as unknown as SurfaceDriver,
  );
  const load = vi.fn(async () => factory);
  const create = () =>
    createTerminalSurface(scope, element, palette, geometry, load);
  return {
    host,
    scope,
    doc,
    style,
    element,
    terminal,
    factory,
    load,
    create,
    pending,
    hooks,
    osc,
    inputDispose,
    input: (text: string) => input?.(text),
  };
}
it("only validated display projection reaches constructor without changing palette or input authority", async () => {
  const f = fixture(),
    surface = await createTerminalSurface(
      f.scope,
      f.element,
      palette,
      {
        ...geometry,
        display: { fontSize: 18, fontFamily: "等宽字体", cursorBlink: true },
      },
      f.load,
    );
  expect(f.factory).toHaveBeenCalledWith(
    expect.objectContaining({
      fontSize: 18,
      fontFamily: "等宽字体",
      cursorBlink: true,
      disableStdin: true,
      theme: expect.objectContaining({ foreground: "#c8c8c8" }),
    }),
  );
  expect(f.terminal.onData).not.toHaveBeenCalled();
  surface.dispose();
});
it("unsafe font projection never invokes loader or creates a terminal", async () => {
  const f = fixture();
  await expect(
    createTerminalSurface(
      f.scope,
      f.element,
      palette,
      {
        ...geometry,
        display: { fontSize: 18, fontFamily: "url(font)", cursorBlink: false },
      },
      f.load,
    ),
  ).rejects.toThrow("view.creation_failed");
  expect(f.load).not.toHaveBeenCalled();
  expect(f.factory).not.toHaveBeenCalled();
});
it("constructs with base palette, trusted nonce document, shared width and query/clipboard suppression before open", async () => {
  const f = fixture(),
    surface = await f.create();
  expect(f.factory).toHaveBeenCalledWith(
    expect.objectContaining({
      cols: 32,
      rows: 8,
      scrollback: 16,
      allowProposedApi: true,
      convertEol: false,
      theme: expect.objectContaining({
        foreground: "#c8c8c8",
        background: "#000000",
      }),
    }),
  );
  const opts = vi.mocked(f.factory).mock.calls[0]?.[0];
  expect(opts?.documentOverride).not.toBe(f.doc);
  opts?.documentOverride?.createElement("style");
  expect(f.style.setAttribute).toHaveBeenCalledWith(
    "nonce",
    "runtime-style-nonce",
  );
  expect(f.terminal.unicode.register).toHaveBeenCalledOnce();
  expect(f.terminal.unicode.activeVersion).not.toBe("");
  expect(f.osc.get(52)?.("remote-read-or-write")).toBe(true);
  expect(f.osc.get(10)?.("?")).toBe(true);
  expect(f.osc.get(10)?.("rgb:aa/bb/cc")).toBe(false);
  expect(f.terminal.onData).not.toHaveBeenCalled();
  surface.dispose();
  f.host.retire(1n);
  expect(f.terminal.dispose).toHaveBeenCalledOnce();
});
it("holds an immutable bounded single-flight write and only resolves at parse callback", async () => {
  const f = fixture(),
    s = await f.create();
  expect(() => s.writeParsed(new Uint8Array(65537))).toThrow("oversize");
  const data = new Uint8Array([65, 66]);
  const work = s.writeParsed(data);
  data[0] = 90;
  expect(f.pending[0]?.text).toBe("AB");
  expect(() => s.writeParsed(data)).toThrow("busy");
  f.pending[0]?.callback();
  await work;
  const next = s.writeParsed(new Uint8Array());
  f.pending[1]?.callback();
  await next;
  f.host.retire(1n);
});
it("manual disposal settles pending parsing without relying on xterm to invoke its cancelled callback", async () => {
  const f = fixture(),
    s = await f.create();
  const work = s.writeParsed(new Uint8Array([65]));
  const rejected = expect(work).rejects.toThrow("retired");
  s.dispose();
  await rejected;
  f.pending[0]?.callback();
  expect(() => s.writeParsed(new Uint8Array())).toThrow("disposed");
  f.host.retire(1n);
  expect(f.terminal.dispose).toHaveBeenCalledOnce();
});
it("refresh settles writes and rejects late input/callbacks; stopped listener cannot run even in current scope", async () => {
  const f = fixture(),
    s = await f.create(),
    handler = vi.fn();
  const stop = s.listenInput(handler);
  f.input("hello");
  stop();
  f.input("after-stop");
  expect(handler).toHaveBeenCalledExactlyOnceWith("hello");
  s.listenInput(handler);
  const work = s.writeParsed(new Uint8Array([65]));
  const rejected = expect(work).rejects.toThrow("retired");
  f.host.begin(2n);
  await rejected;
  f.pending[0]?.callback();
  f.input("old-page");
  expect(handler).toHaveBeenCalledOnce();
  expect(f.terminal.dispose).toHaveBeenCalledOnce();
});
it("late module resolution never constructs or opens a superseded surface", async () => {
  const f = fixture();
  let ready!: (factory: SurfaceFactory) => void;
  const loading = new Promise<SurfaceFactory>((resolve) => {
    ready = resolve;
  });
  const job = createTerminalSurface(
    f.scope,
    f.element,
    palette,
    geometry,
    () => loading,
  );
  await Promise.resolve();
  const rejected = expect(job).rejects.toThrow("retired");
  f.host.begin(2n);
  await rejected;
  ready(f.factory);
  await Promise.resolve();
  await Promise.resolve();
  expect(f.factory).not.toHaveBeenCalled();
});
it("missing nonce, bad geometry or palette cannot construct a terminal", async () => {
  for (const nonce of ["", "__TAURI_STYLE_NONCE__"]) {
    const f = fixture(nonce);
    await expect(f.create()).rejects.toThrow("creation_failed");
    expect(f.load).not.toHaveBeenCalled();
  }
  for (const shape of [
    { ...geometry, columns: 513 },
    { ...geometry, rows: 0 },
    { ...geometry, history: 10001 },
  ]) {
    const f = fixture();
    await expect(
      createTerminalSurface(f.scope, f.element, palette, shape, f.load),
    ).rejects.toThrow("creation_failed");
    expect(f.factory).not.toHaveBeenCalled();
  }
  const f = fixture();
  await expect(
    createTerminalSurface(f.scope, f.element, "{}", geometry, f.load),
  ).rejects.toThrow("creation_failed");
  expect(f.load).not.toHaveBeenCalled();
});
it("failed open unwinds hooks and terminal and retires the failed scope", async () => {
  const f = fixture();
  f.terminal.open.mockImplementation(() => {
    throw new Error("open-fault");
  });
  await expect(f.create()).rejects.toThrow("creation_failed");
  expect(f.terminal.dispose).toHaveBeenCalledOnce();
  for (const hook of f.hooks) expect(hook).toHaveBeenCalledOnce();
  expect(f.scope.isCurrent()).toBe(false);
});
it("holds partial UTF8 outside xterm and decodes it only in stream order", async () => {
  const f = fixture(),
    s = await f.create();
  await s.writeParsed(Uint8Array.of(0xc3));
  expect(f.terminal.write).not.toHaveBeenCalled();
  const work = s.writeParsed(Uint8Array.of(0xa9));
  expect(f.pending[0]?.text).toBe("é");
  f.pending[0]?.callback();
  await work;
  f.host.retire(1n);
});
it("screen-only repair discards incomplete UTF8 without inserting replacement or terminal reset", async () => {
  const f = fixture(),
    s = await f.create();
  await s.writeParsed(Uint8Array.of(0xc3));
  s.beginRepair();
  const work = s.writeParsed(Uint8Array.of(0x18, 65));
  expect(f.pending[0]?.text).toBe("\u0018A");
  f.pending[0]?.callback();
  await work;
  expect(f.terminal.dispose).not.toHaveBeenCalled();
  f.host.retire(1n);
});
it("measures public rendered cells and host bounds, without inventing hidden or zero geometry", async () => {
  const f = fixture(),
    s = await f.create();
  Object.assign(f.doc, { visibilityState: "visible" });
  const host = { ownerDocument: f.doc, clientWidth: 337, clientHeight: 159 };
  let width = 256,
    height = 128;
  const viewport = { offsetWidth: 100, clientWidth: 100 };
  Object.assign(f.terminal, { cols: 32, rows: 8 });
  Object.assign(f.terminal.element, {
    isConnected: true,
    parentElement: host,
    querySelector: (selector: string) =>
      selector === ".xterm-viewport"
        ? viewport
        : { getBoundingClientRect: () => ({ width, height }) },
  });
  expect(s.measuredDimensions()).toEqual({ columns: 42, rows: 9 });
  expect(Object.isFrozen(s.measuredDimensions())).toBe(true);
  viewport.clientWidth = 85;
  expect(s.measuredDimensions()).toEqual({ columns: 40, rows: 9 });
  viewport.clientWidth = 100;
  width = 0;
  expect(s.measuredDimensions()).toBeUndefined();
  width = 256;
  height = 0;
  expect(s.measuredDimensions()).toBeUndefined();
  height = 128;
  host.clientWidth = 0;
  expect(s.measuredDimensions()).toBeUndefined();
  host.clientWidth = 337;
  Object.assign(f.doc, { visibilityState: "hidden" });
  expect(s.measuredDimensions()).toBeUndefined();
  f.host.retire(1n);
  expect(() => s.measuredDimensions()).toThrow("view.retired");
});

it("armed input blocks paste without clipboard reads and removes the interceptor on retirement", async () => {
  const f = fixture(),
    s = await f.create();
  const listener = vi.fn();
  s.enableInput(listener);
  const event = new Event("paste", { cancelable: true });
  expect(f.terminal.element.dispatchEvent(event)).toBe(false);
  expect(event.defaultPrevented).toBe(true);
  expect(listener).not.toHaveBeenCalled();
  f.host.retire(1n);
  const late = new Event("paste", { cancelable: true });
  expect(f.terminal.element.dispatchEvent(late)).toBe(true);
  expect(f.terminal.options.disableStdin).toBe(true);
});

it("explicit confirmed input arm enables stdin, focuses, and teardown suppresses stale events", async () => {
  const f = fixture(),
    surface = await f.create();
  const received = vi.fn();
  expect(f.terminal.options.disableStdin).toBe(true);
  const stop = surface.enableInput(received);
  expect(f.terminal.options.disableStdin).toBe(false);
  expect(f.terminal.focus).toHaveBeenCalledOnce();
  f.input("x");
  expect(received).toHaveBeenCalledExactlyOnceWith("x");
  stop();
  expect(f.terminal.options.disableStdin).toBe(true);
  f.input("old");
  expect(received).toHaveBeenCalledOnce();
  f.host.dispose();
});

it("only true EOF flushes an incomplete UTF8 character and rejects following data", async () => {
  const f = fixture(),
    s = await f.create();
  await s.writeParsed(Uint8Array.of(0xc3));
  const end = s.finishParsed();
  expect(f.pending[0]?.text).toBe("�");
  f.pending[0]?.callback();
  await end;
  expect(() => s.writeParsed(Uint8Array.of(65))).toThrow("already ended");
  f.host.retire(1n);
});
function pasteHandler(f: ReturnType<typeof fixture>) {
  const hook = vi.spyOn(f.terminal.element, "addEventListener");
  return {
    hook,
    event: (text: string, trusted = true) => {
      const handler = hook.mock.calls.find((call) => call[0] === "paste")?.[1];
      if (typeof handler !== "function") throw Error("missing paste test hook");
      const getData = vi.fn(() => text),
        preventDefault = vi.fn(),
        stopImmediatePropagation = vi.fn();
      handler({
        isTrusted: trusted,
        clipboardData: { getData },
        preventDefault,
        stopImmediatePropagation,
      } as unknown as Event);
      return { getData, preventDefault, stopImmediatePropagation };
    },
  };
}
it("opt-in clipboard receives only real text/plain, uses public paste and existing onData", async () => {
  const f = fixture(),
    s = await f.create(),
    paste = pasteHandler(f),
    input = vi.fn(),
    confirm = vi.fn(async () => true);
  const stop = s.enableInput(input, { confirm });
  const event = paste.event("中文🙂");
  await Promise.resolve();
  expect(event.getData).toHaveBeenCalledExactlyOnceWith("text/plain");
  expect(event.preventDefault).toHaveBeenCalledOnce();
  expect(f.terminal.paste).toHaveBeenCalledExactlyOnceWith("中文🙂");
  expect(input).toHaveBeenCalledExactlyOnceWith("中文🙂");
  expect(confirm).not.toHaveBeenCalled();
  stop();
  f.host.retire(1n);
});
it("synthetic ClipboardEvent cannot read clipboard or delegate data; default OSC52 still suppressed", async () => {
  const f = fixture(),
    s = await f.create(),
    paste = pasteHandler(f),
    confirm = vi.fn(async () => true);
  s.enableInput(vi.fn(), { confirm });
  const event = paste.event("execute\n", false);
  await Promise.resolve();
  expect(event.getData).not.toHaveBeenCalled();
  expect(f.terminal.paste).not.toHaveBeenCalled();
  expect(confirm).not.toHaveBeenCalled();
  expect(f.osc.get(52)?.("c;?")).toBe(true);
  f.host.retire(1n);
});
it("closing input while native decision is pending synchronously revokes it and rejects late acceptance", async () => {
  const f = fixture(),
    s = await f.create(),
    paste = pasteHandler(f),
    rejected = vi.fn();
  let accept!: (v: boolean) => void;
  let signal: AbortSignal | undefined;
  const stop = s.enableInput(vi.fn(), {
    confirm: (_t, _a, abort) => {
      signal = abort;
      return new Promise((resolve) => {
        accept = resolve;
      });
    },
    rejected,
  });
  paste.event("a\nb");
  stop();
  expect(signal?.aborted).toBe(true);
  accept(true);
  await Promise.resolve();
  await Promise.resolve();
  expect(f.terminal.paste).not.toHaveBeenCalled();
  expect(rejected).not.toHaveBeenCalled();
  f.host.retire(1n);
});
it("unsupported/oversize text reports a fixed code without leaking text or poisoning the terminal", async () => {
  const f = fixture(),
    s = await f.create(),
    paste = pasteHandler(f),
    rejected = vi.fn();
  s.enableInput(vi.fn(), { confirm: async () => true, rejected });
  paste.event("secret\x1b[201~");
  await Promise.resolve();
  await Promise.resolve();
  expect(rejected).toHaveBeenCalledWith("paste.unsupported_control");
  expect(f.terminal.paste).not.toHaveBeenCalled();
  expect(f.terminal.dispose).not.toHaveBeenCalled();
  f.host.retire(1n);
});
it("Ctrl+C with selection bypasses terminal encoding, without selection remains normal interrupt", async () => {
  const f = fixture();
  await f.create();
  const handler = f.terminal.attachCustomKeyEventHandler.mock
    .calls[0]?.[0] as unknown as (event: KeyboardEvent) => boolean;
  const event = { type: "keydown", key: "c", ctrlKey: true } as KeyboardEvent;
  expect(handler(event)).toBe(true);
  f.terminal.hasSelection.mockReturnValue(true);
  expect(handler(event)).toBe(false);
  expect(handler({ ...event, altKey: true } as KeyboardEvent)).toBe(true);
  f.host.retire(1n);
  expect(handler(event)).toBe(false);
});
it("trusted copy of a readonly selection uses only text/plain; synthetic/stale never writes clipboard", async () => {
  const f = fixture(),
    hook = vi.spyOn(f.terminal.element, "addEventListener");
  await f.create();
  f.terminal.hasSelection.mockReturnValue(true);
  f.terminal.getSelection.mockReturnValue("selected 中文");
  const handler = hook.mock.calls.find((call) => call[0] === "copy")?.[1];
  if (typeof handler !== "function") throw Error("missing copy test hook");
  const setData = vi.fn(),
    event = {
      isTrusted: true,
      clipboardData: { setData },
      preventDefault: vi.fn(),
      stopImmediatePropagation: vi.fn(),
    };
  handler(event as unknown as Event);
  expect(setData).toHaveBeenCalledExactlyOnceWith(
    "text/plain",
    "selected 中文",
  );
  expect(f.terminal.options.disableStdin).toBe(true);
  handler({ ...event, isTrusted: false } as unknown as Event);
  expect(setData).toHaveBeenCalledTimes(1);
  f.host.retire(1n);
  handler(event as unknown as Event);
  expect(setData).toHaveBeenCalledTimes(1);
});

it("bundled font readiness precedes construction and late font completion cannot open a retired scope", async () => {
  const f = fixture();
  let ready!: (faces: FontFace[]) => void;
  const pending = new Promise<FontFace[]>((resolve) => {
    ready = resolve;
  });
  Object.assign(f.element.ownerDocument, {
    fonts: { load: vi.fn(() => pending), check: () => true },
  });
  const job = createTerminalSurface(
    f.scope,
    f.element,
    palette,
    {
      ...geometry,
      display: {
        fontFamily: "JetBrains Mono",
        fontSize: 14,
        cursorBlink: false,
      },
    },
    f.load,
  );
  const rejected = expect(job).rejects.toThrow("retired");
  await Promise.resolve();
  expect(f.factory).not.toHaveBeenCalled();
  f.host.begin(2n);
  await rejected;
  ready([{} as FontFace]);
  await Promise.resolve();
  expect(f.factory).not.toHaveBeenCalled();
  expect(f.terminal.open).not.toHaveBeenCalled();
});
