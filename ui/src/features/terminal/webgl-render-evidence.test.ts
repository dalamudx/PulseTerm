import { afterEach, expect, it, vi } from "vitest";
import { GenerationViewHost } from "./view-host";
import {
  type LigatureAddon,
  type LigatureTerminal,
  WebglRenderEvidence,
} from "./webgl-render-evidence";

afterEach(() => vi.useRealTimers());
function fixture() {
  const life = new AbortController(),
    failed = vi.fn();
  const host = new GenerationViewHost(life.signal, failed),
    scope = host.begin(1n);
  let render: (range: { start: number; end: number }) => void = () => {};
  let loss: () => void = () => {};
  const events = new Map<string, () => void>();
  const gl = {
    NO_ERROR: 0,
    SYNC_GPU_COMMANDS_COMPLETE: 1,
    ALREADY_SIGNALED: 2,
    CONDITION_SATISFIED: 3,
    TIMEOUT_EXPIRED: 4,
    isContextLost: vi.fn(() => false),
    getError: vi.fn(() => 0),
    fenceSync: vi.fn(() => ({})),
    flush: vi.fn(),
    deleteSync: vi.fn(),
    clientWaitSync: vi.fn(() => 2),
  };
  const canvas = {
    width: 640,
    height: 400,
    isConnected: true,
    getContext: vi.fn(() => gl),
    addEventListener: vi.fn((name: string, callback: () => void) =>
      events.set(name, callback),
    ),
    removeEventListener: vi.fn((name: string) => events.delete(name)),
  };
  const atlas = { width: 512, height: 512 } as HTMLCanvasElement;
  const addon = {
    textureAtlas: atlas,
    activate: vi.fn(),
    dispose: vi.fn(),
    onContextLoss: vi.fn((fn: () => void) => {
      loss = fn;
      return { dispose: vi.fn() };
    }),
  } satisfies LigatureAddon;
  const terminal = {
    rows: 20,
    element: {
      ownerDocument: { defaultView: { devicePixelRatio: 1 } },
      classList: { add: vi.fn(), remove: vi.fn() },
      querySelector: () => ({
        getBoundingClientRect: () => ({ width: 640, height: 400 }),
      }),
      querySelectorAll: () => [canvas],
    },
    loadAddon: vi.fn(),
    registerCharacterJoiner: vi.fn(() => 7),
    deregisterCharacterJoiner: vi.fn(),
    onRender: vi.fn((fn: typeof render) => {
      render = fn;
      return { dispose: vi.fn() };
    }),
  };
  const load = vi.fn(async () => addon);
  const create = () =>
    WebglRenderEvidence.create(
      scope,
      terminal as unknown as LigatureTerminal,
      load,
    );
  return {
    host,
    scope,
    failed,
    gl,
    canvas,
    addon,
    terminal,
    load,
    create,
    render: (start = 0, end = 19) => render({ start, end }),
    loss: () => loss(),
    nativeLoss: () => events.get("webglcontextlost")?.(),
  };
}
it("rejects missing APIs without loading an addon and never silently uses DOM", async () => {
  const f = fixture();
  const invalid = {
    element: f.terminal.element,
  } as unknown as LigatureTerminal;
  await expect(
    WebglRenderEvidence.create(f.scope, invalid, f.load),
  ).rejects.toThrow("webgl_api_unavailable");
  expect(f.load).not.toHaveBeenCalled();
  f.canvas.getContext.mockReturnValue(null as never);
  await expect(f.create()).rejects.toThrow("webgl_unavailable");
  expect(f.addon.dispose).toHaveBeenCalledOnce();
  expect(f.terminal.registerCharacterJoiner).not.toHaveBeenCalled();
});
it("requires current full-row render plus GPU completion, never parser success", async () => {
  const f = fixture(),
    evidence = f.scope.own(await f.create());
  evidence.prepare();
  f.render(0, 10);
  await expect(evidence.verify()).rejects.toThrow("webgl_not_drawn");
  expect(f.gl.fenceSync).not.toHaveBeenCalled();
  f.render();
  await evidence.verify();
  expect(f.gl.flush).toHaveBeenCalledOnce();
  expect(f.gl.deleteSync).toHaveBeenCalledOnce();
  await expect(evidence.verify()).rejects.toThrow("webgl_not_drawn");
  f.host.dispose();
  expect(f.terminal.deregisterCharacterJoiner).toHaveBeenCalledWith(7);
  expect(f.addon.dispose).toHaveBeenCalledOnce();
  expect(f.terminal.element.classList.add).toHaveBeenCalledWith(
    "pulseterm-ligatures",
  );
  expect(f.terminal.element.classList.remove).toHaveBeenCalledWith(
    "pulseterm-ligatures",
  );
});
it("immediate native context loss cancels the GPU wait and invokes original failure once", async () => {
  const f = fixture(),
    evidence = f.scope.own(await f.create());
  f.gl.clientWaitSync.mockReturnValue(4);
  evidence.prepare();
  f.render();
  const job = evidence.verify(),
    rejected = expect(job).rejects.toThrow("retired");
  f.nativeLoss();
  await rejected;
  f.loss();
  expect(f.scope.isCurrent()).toBe(false);
  expect(f.failed).toHaveBeenCalledOnce();
  expect(f.failed).toHaveBeenCalledWith(
    expect.objectContaining({ message: "terminal.webgl_context_lost" }),
  );
  expect(f.addon.dispose).toHaveBeenCalledOnce();
  expect(f.gl.deleteSync).toHaveBeenCalledOnce();
});
it("late load and old context loss cannot touch a replacement generation", async () => {
  const f = fixture();
  let finish!: (value: LigatureAddon) => void;
  const job = WebglRenderEvidence.create(
    f.scope,
    f.terminal as unknown as LigatureTerminal,
    () =>
      new Promise((ok) => {
        finish = ok;
      }),
  );
  const next = f.host.begin(2n);
  finish(f.addon);
  await expect(job).rejects.toThrow("retired");
  expect(f.terminal.loadAddon).not.toHaveBeenCalled();
  expect(f.addon.dispose).toHaveBeenCalledOnce();
  f.scope.invalidate(Error("old context"));
  expect(next.isCurrent()).toBe(true);
  expect(f.failed).not.toHaveBeenCalled();
  f.host.dispose();
});
it("GPU waits time out within their own bound and release the fence", async () => {
  vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout", "performance"] });
  const f = fixture(),
    evidence = f.scope.own(await f.create());
  f.gl.clientWaitSync.mockReturnValue(4);
  evidence.prepare();
  f.render();
  const rejected = expect(evidence.verify()).rejects.toThrow(
    "webgl_draw_timeout",
  );
  await vi.advanceTimersByTimeAsync(1000);
  await rejected;
  expect(f.gl.deleteSync).toHaveBeenCalledOnce();
  expect(vi.getTimerCount()).toBe(0);
  f.host.dispose();
});
