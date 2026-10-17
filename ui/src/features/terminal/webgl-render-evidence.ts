import type { IDisposable, ITerminalAddon, Terminal } from "@xterm/xterm";
import { operatorLigatures } from "./operator-ligatures";
import type { ViewScope } from "./view-host";

export interface LigatureAddon extends ITerminalAddon {
  readonly onContextLoss: (listener: () => void) => IDisposable;
  readonly textureAtlas?: HTMLCanvasElement;
  readonly onAddTextureAtlasCanvas?: (
    listener: (canvas: HTMLCanvasElement) => void,
  ) => IDisposable;
  readonly onRemoveTextureAtlasCanvas?: (
    listener: (canvas: HTMLCanvasElement) => void,
  ) => IDisposable;
}
export type WebglLoader = () => Promise<LigatureAddon>;
export type LigatureTerminal = Pick<
  Terminal,
  | "element"
  | "rows"
  | "loadAddon"
  | "onRender"
  | "registerCharacterJoiner"
  | "deregisterCharacterJoiner"
>;
async function loadWebgl(): Promise<LigatureAddon> {
  const { WebglAddon } = await import("@xterm/addon-webgl");
  return new WebglAddon(true);
}

/** Render events and a completed GPU fence are drawing evidence, not a desktop
 * compositor acknowledgement. This grants no input or backend authority. */
export class WebglRenderEvidence {
  private disposed = false;
  private rendered = false;
  private requested = false;
  private pending: (() => void) | undefined;
  private readonly cleanup: (() => void)[] = [];
  private readonly atlas = new Set<HTMLCanvasElement>();
  private canvas!: HTMLCanvasElement;
  private gl!: WebGL2RenderingContext;
  private constructor(
    private readonly scope: ViewScope,
    private readonly terminal: LigatureTerminal,
    private readonly addon: LigatureAddon,
  ) {}

  static async create(
    scope: ViewScope,
    terminal: LigatureTerminal,
    load: WebglLoader = loadWebgl,
  ): Promise<WebglRenderEvidence> {
    if (
      !terminal.loadAddon ||
      !terminal.onRender ||
      !terminal.registerCharacterJoiner ||
      !terminal.deregisterCharacterJoiner
    )
      throw Error("terminal.webgl_api_unavailable");
    const addon = await load();
    const result = new WebglRenderEvidence(scope, terminal, addon);
    try {
      result.check();
      const root = terminal.element;
      const screen = root?.querySelector<HTMLElement>(".xterm-screen");
      const bounds = screen?.getBoundingClientRect();
      const dpr = root?.ownerDocument.defaultView?.devicePixelRatio ?? 1;
      if (
        !bounds ||
        bounds.width <= 0 ||
        bounds.height <= 0 ||
        !Number.isFinite(dpr) ||
        dpr <= 0 ||
        bounds.width * bounds.height * dpr * dpr > 4194304
      )
        throw Error("terminal.webgl_geometry_budget");
      const lost = () => scope.invalidate(Error("terminal.webgl_context_lost"));
      const loss = addon.onContextLoss(lost);
      result.cleanup.push(() => loss.dispose());
      const added = addon.onAddTextureAtlasCanvas?.((canvas) =>
        result.trackAtlas(canvas),
      );
      if (added) result.cleanup.push(() => added.dispose());
      const removed = addon.onRemoveTextureAtlasCanvas?.((canvas) =>
        result.atlas.delete(canvas),
      );
      if (removed) result.cleanup.push(() => removed.dispose());
      if (!root) throw Error("terminal.webgl_api_unavailable");
      root.classList.add("pulseterm-ligatures");
      result.cleanup.push(() => root.classList.remove("pulseterm-ligatures"));
      terminal.loadAddon(addon);
      result.check();
      if (addon.textureAtlas) result.trackAtlas(addon.textureAtlas);
      for (const canvas of root?.querySelectorAll<HTMLCanvasElement>(
        ".xterm-screen > canvas",
      ) ?? []) {
        const gl = canvas.getContext("webgl2");
        if (gl) {
          result.canvas = canvas;
          result.gl = gl;
          break;
        }
      }
      if (!result.gl) throw Error("terminal.webgl_unavailable");
      result.canvas.addEventListener("webglcontextlost", lost);
      result.cleanup.push(() =>
        result.canvas.removeEventListener("webglcontextlost", lost),
      );
      const joiner = terminal.registerCharacterJoiner(operatorLigatures);
      result.cleanup.push(() => terminal.deregisterCharacterJoiner(joiner));
      const render = terminal.onRender(({ start, end }) => {
        if (
          result.requested &&
          start === 0 &&
          end >= terminal.rows - 1 &&
          scope.isCurrent()
        )
          result.rendered = true;
      });
      result.cleanup.push(() => render.dispose());
      return result;
    } catch (error) {
      result.dispose();
      throw error;
    }
  }
  private trackAtlas(canvas: HTMLCanvasElement): void {
    this.atlas.add(canvas);
    let bytes = 0;
    for (const page of this.atlas) bytes += page.width * page.height * 4;
    if (this.atlas.size > 4 || bytes > 67108864) {
      this.scope.invalidate(Error("terminal.webgl_atlas_budget"));
      throw Error("terminal.webgl_atlas_budget");
    }
  }
  private check(): void {
    if (this.disposed || !this.scope.isCurrent()) throw Error("view.retired");
  }
  prepare(): void {
    this.check();
    if (this.pending) throw Error("terminal.webgl_draw_busy");
    this.requested = true;
    this.rendered = false;
  }
  async verify(): Promise<void> {
    this.check();
    const gl = this.gl;
    if (
      !this.requested ||
      !this.rendered ||
      !this.canvas.isConnected ||
      !this.addon.textureAtlas ||
      gl.isContextLost() ||
      this.canvas.width <= 0 ||
      this.canvas.height <= 0 ||
      this.canvas.width * this.canvas.height > 4194304 ||
      gl.getError() !== gl.NO_ERROR
    )
      throw Error("terminal.webgl_not_drawn");
    this.requested = false;
    if (this.pending) throw Error("terminal.webgl_draw_busy");
    const fence = gl.fenceSync(gl.SYNC_GPU_COMMANDS_COMPLETE, 0);
    if (!fence) throw Error("terminal.webgl_fence_failed");
    gl.flush();
    await new Promise<void>((resolve, reject) => {
      const started = performance.now();
      let timer: ReturnType<typeof setTimeout> | undefined;
      let done = false;
      const finish = (error?: Error) => {
        if (done) return;
        done = true;
        clearTimeout(timer);
        this.scope.signal.removeEventListener("abort", abort);
        this.pending = undefined;
        gl.deleteSync(fence);
        if (error) reject(error);
        else resolve();
      };
      const abort = () => finish(Error("view.retired"));
      const step = () => {
        try {
          this.check();
          if (gl.isContextLost()) throw Error("terminal.webgl_context_lost");
          if (performance.now() - started >= 1000)
            throw Error("terminal.webgl_draw_timeout");
          const status = gl.clientWaitSync(fence, 0, 0);
          if (
            status === gl.ALREADY_SIGNALED ||
            status === gl.CONDITION_SATISFIED
          )
            finish();
          else if (status === gl.TIMEOUT_EXPIRED) timer = setTimeout(step, 4);
          else throw Error("terminal.webgl_fence_failed");
        } catch (error) {
          finish(
            error instanceof Error
              ? error
              : Error("terminal.webgl_fence_failed"),
          );
        }
      };
      this.pending = abort;
      this.scope.signal.addEventListener("abort", abort, { once: true });
      step();
    });
    this.check();
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.pending?.();
    let failed = false;
    for (const cleanup of this.cleanup.reverse()) {
      try {
        cleanup();
      } catch {
        failed = true;
      }
    }
    try {
      this.addon.dispose();
    } catch {
      failed = true;
    }
    if (failed) throw Error("terminal.webgl_cleanup_failed");
  }
}
