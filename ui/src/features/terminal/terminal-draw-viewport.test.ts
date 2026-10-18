import { expect, it, vi } from "vitest";
import { type SurfaceDriver, TerminalSurface } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";

vi.mock("./terminal-scroll-return", () => ({
  createScrollReturn: () => ({ refresh: vi.fn(), dispose: vi.fn() }),
}));

function fixture() {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn());
  const scope = host.begin(1n);
  let scroll = () => {};
  const rows: [{ textContent: string }, { textContent: string }] = [
    { textContent: "line-10" },
    { textContent: "line-11" },
  ];
  const buffer = {
    viewportY: 10,
    baseY: 10,
    getLine: (line: number) => ({ translateToString: () => `line-${line}` }),
  };
  const terminal = {
    cols: 10,
    rows: 2,
    buffer: { active: buffer },
    options: { disableStdin: true },
    refresh: vi.fn(),
    dispose: vi.fn(),
    scrollToBottom: vi.fn(),
    onScroll: (listener: () => void) => {
      scroll = listener;
      return { dispose: vi.fn() };
    },
    element: {
      isConnected: true,
      ownerDocument: { defaultView: { innerWidth: 1000, innerHeight: 800 } },
      querySelector: (selector: string) =>
        selector === ".xterm-screen"
          ? {
              getBoundingClientRect: () => ({
                top: 10,
                left: 10,
                width: 100,
                height: 40,
                bottom: 50,
                right: 110,
              }),
            }
          : { children: rows },
    },
  };
  const surface = new TerminalSurface(
    scope,
    terminal as unknown as SurfaceDriver,
    vi.fn(),
  );
  surface.bindScrollReturn({} as HTMLElement);
  return { host, surface, terminal, buffer, rows, scroll: () => scroll() };
}

it("a moved viewport requires a fresh draw before its DOM projection is accepted", () => {
  const f = fixture();
  try {
    f.surface.prepareDraw();
    expect(f.surface.drawIsCurrent()).toBe(true);
    f.buffer.viewportY = 4;
    f.scroll();
    expect(f.surface.drawIsCurrent()).toBe(false);
    expect(() => f.surface.verifyDraw()).toThrow("terminal.dom_projection");
    f.surface.prepareDraw();
    f.rows[0].textContent = "line-4";
    f.rows[1].textContent = "line-5";
    expect(f.surface.drawIsCurrent()).toBe(true);
    expect(() => f.surface.verifyDraw()).not.toThrow();
    expect(f.terminal.options.disableStdin).toBe(true);
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});

it("scrolling away and back still invalidates the pending draw", () => {
  const f = fixture();
  try {
    f.surface.prepareDraw();
    f.buffer.viewportY = 4;
    f.scroll();
    f.buffer.viewportY = 10;
    f.scroll();
    expect(f.surface.drawIsCurrent()).toBe(false);
    f.surface.prepareDraw();
    expect(f.surface.drawIsCurrent()).toBe(true);
    f.terminal.buffer.active = { ...f.buffer };
    expect(f.surface.drawIsCurrent()).toBe(false);
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});

it("stable viewport corruption still fails, and a retired surface cannot validate a draw", () => {
  const f = fixture();
  try {
    f.surface.prepareDraw();
    f.rows[1].textContent = "wrong-row";
    expect(f.surface.drawIsCurrent()).toBe(true);
    expect(() => f.surface.verifyDraw()).toThrow("terminal.dom_projection");
    f.surface.dispose();
    expect(() => f.surface.drawIsCurrent()).toThrow(
      "terminal.surface_disposed",
    );
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});
