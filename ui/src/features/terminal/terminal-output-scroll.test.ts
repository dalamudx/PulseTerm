import { expect, it, vi } from "vitest";
import {
  createTerminalSurface,
  type SurfaceDriver,
  type SurfaceFactory,
} from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

function fixture() {
  const controller = new AbortController();
  const host = new GenerationViewHost(controller.signal, vi.fn());
  const scope = host.begin(1n);
  const pending: (() => void)[] = [];
  const hook = () => ({ dispose: vi.fn() });
  const scroll = vi.fn();
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
    write: vi.fn((_text: string, done: () => void) => pending.push(done)),
    scrollToBottom: scroll as (() => void) | undefined,
  };
  const document = {
    querySelector: () => ({ nonce: "runtime-style-nonce" }),
    createElement: () => ({ setAttribute: vi.fn() }),
  } as unknown as Document;
  const element = { ownerDocument: document } as HTMLElement;
  const palette = JSON.stringify({
    version: 1,
    palette: {
      background: [0, 0, 0],
      foreground: [255, 255, 255],
      cursor: [255, 255, 255],
      ansi: Array.from({ length: 16 }, () => [20, 30, 40]),
    },
  });
  const factory: SurfaceFactory = () => terminal as unknown as SurfaceDriver;
  const make = (scrollOnOutput?: boolean) =>
    createTerminalSurface(
      scope,
      element,
      palette,
      {
        columns: 32,
        rows: 8,
        history: 16,
        display: {
          fontSize: 14,
          fontFamily: "monospace",
          cursorBlink: false,
          ...(scrollOnOutput === undefined ? {} : { scrollOnOutput }),
        },
      },
      async () => factory,
    );
  return { controller, host, pending, terminal, scroll, make };
}

it.each([undefined, false, true])(
  "output scroll %s waits for parsing and never enables input",
  async (enabled) => {
    const f = fixture();
    const surface = await f.make(enabled);
    try {
      const written = surface.writeParsed(
        new TextEncoder().encode("output\r\n"),
      );
      expect(f.scroll).not.toHaveBeenCalled();
      f.pending[0]?.();
      await written;
      f.pending[0]?.();
      expect(f.scroll).toHaveBeenCalledTimes(enabled === true ? 1 : 0);
      expect(f.terminal.options.disableStdin).toBe(true);
      expect(f.terminal.onData).not.toHaveBeenCalled();
    } finally {
      surface.dispose();
      f.host.dispose();
    }
  },
);
it.each(["abort", "dispose"])(
  "late parse completion after %s cannot scroll",
  async (action) => {
    const f = fixture();
    const surface = await f.make(true);
    try {
      const written = surface.writeParsed(new TextEncoder().encode("late"));
      const rejected = expect(written).rejects.toThrow("view.retired");
      if (action === "abort") f.controller.abort();
      else surface.dispose();
      await rejected;
      f.pending[0]?.();
      expect(f.scroll).not.toHaveBeenCalled();
    } finally {
      surface.dispose();
      f.host.dispose();
    }
  },
);
it("incomplete UTF-8 and empty EOF do not force a viewport transition", async () => {
  const f = fixture();
  const surface = await f.make(true);
  try {
    const bytes = new TextEncoder().encode("你");
    await surface.writeParsed(bytes.slice(0, 2));
    expect(f.pending).toHaveLength(0);
    const written = surface.writeParsed(bytes.slice(2));
    f.pending[0]?.();
    await written;
    await surface.finishParsed();
    expect(f.scroll).toHaveBeenCalledTimes(1);
  } finally {
    surface.dispose();
    f.host.dispose();
  }
});
it("a scrolling failure rejects write completion", async () => {
  const f = fixture();
  const surface = await f.make(true);
  try {
    f.scroll.mockImplementation(() => {
      throw Error("renderer failed");
    });
    const written = surface.writeParsed(new TextEncoder().encode("line"));
    const rejected = expect(written).rejects.toThrow("terminal.scroll_failed");
    f.pending[0]?.();
    await rejected;
  } finally {
    surface.dispose();
    f.host.dispose();
  }
});
it("enabled following requires the public scroll API", async () => {
  const f = fixture();
  f.terminal.scrollToBottom = undefined;
  try {
    await expect(f.make(true)).rejects.toThrow("view.creation_failed");
    expect(f.terminal.open).not.toHaveBeenCalled();
    expect(f.terminal.dispose).toHaveBeenCalledOnce();
  } finally {
    f.host.dispose();
  }
});
