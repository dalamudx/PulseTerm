import { expect, it, vi } from "vitest";
import { createTerminalSurface, type SurfaceDriver } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

async function fixture(
  backspaceSend: "del" | "bs" = "bs",
  deleteSend: "csi" | "del" = "csi",
) {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n);
  const root = new EventTarget();
  let handler: (event: KeyboardEvent) => boolean = () => true;
  let data: (text: string) => void = () => {};
  const hook = () => ({ dispose: vi.fn() });
  const terminal = {
    options: { disableStdin: true },
    element: root,
    parser: {
      registerCsiHandler: hook,
      registerDcsHandler: hook,
      registerOscHandler: hook,
    },
    unicode: { register: vi.fn(), activeVersion: "" },
    open: vi.fn(),
    dispose: vi.fn(),
    focus: vi.fn(),
    hasSelection: () => false,
    getSelection: () => "",
    attachCustomKeyEventHandler: (fn: typeof handler) => {
      handler = fn;
    },
    onData: (fn: typeof data) => {
      data = fn;
      return { dispose: vi.fn() };
    },
    input: vi.fn((text: string, _user?: boolean) => data(text)),
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
    {
      columns: 32,
      rows: 8,
      history: 16,
      display: {
        fontSize: 14,
        fontFamily: "monospace",
        cursorBlink: false,
        backspaceSend,
        deleteSend,
      },
    },
    async () => () => terminal as unknown as SurfaceDriver,
  );
  const event = (patch: Record<string, unknown> = {}) =>
    ({
      type: "keydown",
      key: "Backspace",
      keyCode: 8,
      isTrusted: true,
      defaultPrevented: false,
      isComposing: false,
      ctrlKey: false,
      altKey: false,
      metaKey: false,
      shiftKey: false,
      preventDefault: vi.fn(),
      ...patch,
    }) as unknown as KeyboardEvent;
  return {
    host,
    root,
    surface,
    terminal,
    event,
    press: (e: KeyboardEvent) => handler(e),
    emit: (text: string) => data(text),
  };
}

it("BS override is armed only by enableInput and uses the original onData path once", async () => {
  const f = await fixture();
  const send = vi.fn();
  try {
    expect(f.press(f.event())).toBe(true);
    expect(f.terminal.input).not.toHaveBeenCalled();
    const close = f.surface.enableInput(send);
    const event = f.event();
    expect(f.press(event)).toBe(false);
    expect(event.preventDefault).toHaveBeenCalledOnce();
    expect(f.terminal.input).toHaveBeenCalledExactlyOnceWith("\b", true);
    expect(send).toHaveBeenCalledExactlyOnceWith("\b");
    close();
    expect(f.press(f.event())).toBe(true);
    expect(f.terminal.options.disableStdin).toBe(true);
    f.emit("late");
    expect(send).toHaveBeenCalledTimes(1);
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});

it("DEL leaves xterm and arbitrary text unchanged, including DEL bytes", async () => {
  const f = await fixture("del");
  const send = vi.fn();
  try {
    f.surface.enableInput(send);
    expect(f.press(f.event())).toBe(true);
    expect(f.terminal.input).not.toHaveBeenCalled();
    f.emit("中文\x7ftext");
    expect(send).toHaveBeenCalledExactlyOnceWith("中文\x7ftext");
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});

it("composition, modifiers, prevented and synthetic events never use the override", async () => {
  const f = await fixture();
  try {
    f.surface.enableInput(vi.fn());
    for (const patch of [
      { ctrlKey: true },
      { altKey: true },
      { metaKey: true },
      { shiftKey: true },
      { isTrusted: false },
      { defaultPrevented: true },
      { isComposing: true },
      { keyCode: 229 },
      { key: "Process" },
      { type: "keyup" },
    ])
      expect(f.press(f.event(patch))).toBe(true);
    f.root.dispatchEvent(new Event("compositionstart"));
    expect(f.press(f.event())).toBe(true);
    f.root.dispatchEvent(new Event("compositionend"));
    f.terminal.options.disableStdin = true;
    expect(f.press(f.event())).toBe(true);
    expect(f.terminal.input).not.toHaveBeenCalled();
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});

it("retired or disposed Surface cannot submit Backspace or late data", async () => {
  for (const retire of [true, false]) {
    const f = await fixture();
    const send = vi.fn();
    try {
      f.surface.enableInput(send);
      if (retire) f.host.begin(2n);
      else f.surface.dispose();
      expect(f.press(f.event())).toBe(false);
      f.emit("late");
      expect(send).not.toHaveBeenCalled();
      expect(f.terminal.input).not.toHaveBeenCalled();
    } finally {
      f.surface.dispose();
      f.host.dispose();
    }
  }
});

it("Delete DEL uses the same gate, preserves modified keys and defaults to CSI", async () => {
  const f = await fixture("del", "del"),
    send = vi.fn();
  try {
    expect(f.press(f.event({ key: "Delete" }))).toBe(true);
    const close = f.surface.enableInput(send);
    expect(f.press(f.event({ key: "Delete" }))).toBe(false);
    expect(send).toHaveBeenCalledExactlyOnceWith("\x7f");
    for (const extra of [
      { altKey: true },
      { ctrlKey: true },
      { metaKey: true },
      { shiftKey: true },
      { isTrusted: false },
      { isComposing: true },
      { keyCode: 229 },
    ])
      expect(f.press(f.event({ key: "Delete", ...extra }))).toBe(true);
    f.emit("literal\x7f");
    expect(send).toHaveBeenLastCalledWith("literal\x7f");
    close();
    expect(f.press(f.event({ key: "Delete" }))).toBe(true);
    f.host.begin(2n);
    expect(f.press(f.event({ key: "Delete" }))).toBe(false);
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
  const defaults = await fixture();
  try {
    defaults.surface.enableInput(vi.fn());
    expect(defaults.press(defaults.event({ key: "Delete" }))).toBe(true);
    expect(defaults.terminal.input).not.toHaveBeenCalled();
  } finally {
    defaults.surface.dispose();
    defaults.host.dispose();
  }
});
