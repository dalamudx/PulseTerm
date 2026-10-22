import { expect, it, vi } from "vitest";
import { type SurfaceDriver, TerminalSurface } from "./terminal-surface";
import { GenerationViewHost } from "./view-host";
import { WebglRenderEvidence } from "./webgl-render-evidence";

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

it("stable DOM output keeps the parser's dirty row without expanding to a full redraw", () => {
  const f = fixture();
  try {
    f.surface.prepareDraw();
    expect(f.terminal.refresh.mock.calls).toEqual([[0, 1]]);
    f.terminal.refresh.mockClear();
    // The locked xterm parser schedules its affected row before write completion.
    f.terminal.refresh(1, 1);
    f.surface.prepareDraw();
    expect(f.terminal.refresh.mock.calls).toEqual([[1, 1]]);
    expect(f.surface.drawIsCurrent()).toBe(true);
    expect(() => f.surface.verifyDraw()).not.toThrow();
    f.rows[1].textContent = "stale";
    expect(() => f.surface.verifyDraw()).toThrow("terminal.dom_projection");
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});
it("repair and viewport identity changes retain full refresh while stable draws do not", () => {
  const f = fixture();
  try {
    f.surface.prepareDraw();
    f.terminal.refresh.mockClear();
    f.surface.prepareDraw();
    expect(f.terminal.refresh).not.toHaveBeenCalled();
    f.surface.beginRepair();
    f.surface.prepareDraw();
    expect(f.terminal.refresh).toHaveBeenLastCalledWith(0, 1);
    f.terminal.refresh.mockClear();
    f.terminal.buffer.active = { ...f.buffer };
    f.surface.prepareDraw();
    expect(f.terminal.refresh).toHaveBeenCalledOnce();
    f.terminal.refresh.mockClear();
    f.terminal.cols = 11;
    f.surface.prepareDraw();
    expect(f.terminal.refresh).toHaveBeenCalledOnce();
  } finally {
    f.surface.dispose();
    f.host.dispose();
  }
});
it("WebGL still requests full rows for every GPU evidence fence on a stable viewport", async () => {
  const f = fixture();
  const gpu = { prepare: vi.fn(), verify: vi.fn(), dispose: vi.fn() };
  const create = vi
    .spyOn(WebglRenderEvidence, "create")
    .mockResolvedValue(gpu as unknown as WebglRenderEvidence);
  try {
    await f.surface.enableLigatures();
    f.surface.prepareDraw();
    f.surface.prepareDraw();
    expect(f.terminal.refresh.mock.calls).toEqual([
      [0, 1],
      [0, 1],
    ]);
    expect(gpu.prepare).toHaveBeenCalledTimes(2);
    expect(f.terminal.options.disableStdin).toBe(true);
  } finally {
    create.mockRestore();
    f.surface.dispose();
    f.host.dispose();
  }
});
