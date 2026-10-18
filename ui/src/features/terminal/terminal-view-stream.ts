import {
  AttachmentV2Consumer,
  type V2Boundary,
  type V2Event,
  type V2Receipt,
  type V2Sink,
} from "../../ipc/attachment-v2";
import type { DisplayPreferences } from "./display-preferences";
import type { Geometry } from "./fit-scheduler";
import { renderFence } from "./render-fence";
import {
  createTerminalSurface,
  type SurfaceGeometry,
  type TerminalPasteOptions,
  type TerminalSurface,
} from "./terminal-surface";
import type { ViewResource, ViewScope } from "./view-host";

type Checkpoint = Extract<V2Event, { kind: "checkpoint" | "marker" | "end" }>;
export interface DrawnBoundary extends V2Boundary {
  readonly generation: bigint;
  readonly digest: string;
  readonly kind: "capture" | "marker" | "checkpoint" | "end";
}
/** Callbacks MUST use the page/nonce/generation-authorized native adapter. The
 * renderer and its checksum grant no authority; capture is not latest freshness. */
export interface TerminalStreamCallbacks {
  openLink?(url: string): Promise<void>;
  failed?(error: Error): void;
  beforeDraw?(): void;
  acknowledge(generation: bigint, sequence: number): Promise<void>;
  observe(
    boundary: Readonly<DrawnBoundary>,
    signal: AbortSignal,
  ): Promise<void>;
}
type PaintSurface = Pick<
  TerminalSurface,
  | "writeParsed"
  | "finishParsed"
  | "beginRepair"
  | "visibleDigest"
  | "prepareDraw"
  | "verifyDraw"
  | "dispose"
> &
  Partial<
    Pick<
      TerminalSurface,
      "enableInput" | "measuredDimensions" | "focusOutput" | "drawIsCurrent"
    >
  >;
export interface TerminalStreamRenderer {
  create(
    scope: ViewScope,
    element: HTMLElement,
    palette: string,
    geometry: SurfaceGeometry,
  ): Promise<PaintSurface>;
  draw(current?: () => boolean): Promise<void>;
}
class SurfaceSink implements V2Sink {
  private surface: PaintSurface | undefined;
  private geometryState: SurfaceGeometry | undefined;
  private closed = false;
  private live = false;
  constructor(
    private readonly scope: ViewScope,
    private readonly element: HTMLElement,
    private readonly palette: string,
    private readonly history: number,
    private readonly renderer: TerminalStreamRenderer,
    private readonly observed: TerminalStreamCallbacks["observe"],
    private readonly committed: () => void,
    private readonly beforeDraw: (() => void) | undefined,
    private readonly display: DisplayPreferences | undefined,
    private readonly openLink: TerminalStreamCallbacks["openLink"],
  ) {}
  private check(signal: AbortSignal): void {
    if (this.closed || signal.aborted || !this.scope.isCurrent())
      throw new Error("view.retired");
  }
  private get(signal: AbortSignal): PaintSurface {
    this.check(signal);
    if (!this.surface) throw new Error("terminal.before_snapshot");
    return this.surface;
  }
  async reset(boundary: V2Boundary, signal: AbortSignal): Promise<void> {
    this.check(signal);
    if (this.surface) throw new Error("terminal.duplicate_snapshot");
    this.geometryState = Object.freeze({
      columns: boundary.columns,
      rows: boundary.rows,
      history: this.history,
      display: this.display,
      openLink: this.openLink,
    });
    const surface = await this.renderer.create(
      this.scope,
      this.element,
      this.palette,
      this.geometryState,
    );
    if (this.closed || signal.aborted || !this.scope.isCurrent()) {
      surface.dispose();
      throw new Error("view.retired");
    }
    this.surface = surface;
  }
  async write(
    bytes: Uint8Array,
    signal: AbortSignal,
    notifyBell = this.live,
  ): Promise<void> {
    return this.writeBatch([bytes], signal, notifyBell);
  }
  async writeBatch(
    chunks: readonly Uint8Array[],
    signal: AbortSignal,
    notifyBell = this.live,
  ): Promise<void> {
    const surface = this.get(signal);
    let bytes = chunks[0] ?? new Uint8Array(0);
    if (chunks.length > 1) {
      bytes = new Uint8Array(
        chunks.reduce((size, chunk) => size + chunk.length, 0),
      );
      let offset = 0;
      for (const chunk of chunks) {
        bytes.set(chunk, offset);
        offset += chunk.length;
      }
    }
    for (let offset = 0; offset < bytes.length; offset += 65536) {
      this.check(signal);
      await surface.writeParsed(
        bytes.subarray(offset, offset + 65536),
        notifyBell ? signal : undefined,
      );
      this.check(signal);
    }
    await this.draw(signal);
  }
  private async draw(signal: AbortSignal): Promise<void> {
    this.beforeDraw?.();
    const surface = this.get(signal);
    surface.prepareDraw();
    await this.renderer.draw(() => {
      this.check(signal);
      if (surface.drawIsCurrent?.() !== false) return true;
      surface.prepareDraw();
      return false;
    });
    this.beforeDraw?.();
    this.check(signal);
    await surface.verifyDraw();
    this.check(signal);
  }
  async ready(boundary: V2Boundary, signal: AbortSignal): Promise<void> {
    await this.report("capture", boundary, signal);
    this.committed();
    this.live = true;
  }
  async repair(
    boundary: V2Boundary,
    bytes: Uint8Array,
    signal: AbortSignal,
  ): Promise<void> {
    const surface = this.get(signal);
    if (
      boundary.columns !== this.geometryState?.columns ||
      boundary.rows !== this.geometryState.rows
    )
      throw new Error("terminal.repair_size");
    if (bytes[0] !== 0x18) throw new Error("terminal.repair_requires_can");
    surface.beginRepair();
    await this.write(bytes, signal, false);
  }
  geometry(): Readonly<Geometry> {
    if (!this.geometryState) throw new Error("terminal.before_snapshot");
    return {
      columns: this.geometryState.columns,
      rows: this.geometryState.rows,
    };
  }
  focusOutput(): boolean {
    if (this.closed || !this.scope.isCurrent()) return false;
    return this.surface?.focusOutput?.() ?? false;
  }
  measuredDimensions(): Readonly<Geometry> | undefined {
    return this.get(this.scope.signal).measuredDimensions?.();
  }
  enableInput(
    listener: (text: string) => void,
    paste?: TerminalPasteOptions,
  ): () => void {
    const surface = this.get(this.scope.signal);
    if (!surface.enableInput) throw new Error("input.unsupported");
    return surface.enableInput(listener, paste);
  }
  marker(boundary: Checkpoint, signal: AbortSignal): Promise<void> {
    return this.report("marker", boundary, signal);
  }
  checkpoint(boundary: Checkpoint, signal: AbortSignal): Promise<void> {
    return this.report("checkpoint", boundary, signal);
  }
  async finish(boundary: Checkpoint, signal: AbortSignal): Promise<void> {
    await this.get(signal).finishParsed();
    this.check(signal);
    await this.report("end", boundary, signal);
  }
  private async report(
    kind: DrawnBoundary["kind"],
    boundary: Pick<V2Boundary, "revision" | "sourceBytes" | "utf8Bytes">,
    signal: AbortSignal,
  ): Promise<void> {
    await this.draw(signal);
    const surface = this.get(signal),
      geometry = this.geometryState;
    if (!geometry) throw new Error("terminal.before_snapshot");
    const observation = Object.freeze({
      ...boundary,
      columns: geometry.columns,
      rows: geometry.rows,
      kind,
      generation: this.scope.generation,
      digest: surface.visibleDigest(),
    });
    await this.observed(observation, signal);
    this.check(signal);
  }
  dispose(): void {
    if (this.closed) return;
    this.closed = true;
    this.surface?.dispose();
  }
}
/** Raw ArrayBuffers enter one bounded, version-strict ordered queue. The caller
 * still owns native detach/error reporting. No commands or session creation here. */
export class TerminalViewStream implements ViewResource {
  private readonly sink: SurfaceSink;
  private readonly consumer: AttachmentV2Consumer;
  private disposed = false;
  private cancelling = false;
  private paused = false;
  private ready = false;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly started = performance.now();
  constructor(
    private readonly scope: ViewScope,
    element: HTMLElement,
    palette: string,
    history: number,
    private readonly callbacks: TerminalStreamCallbacks,
    renderer: TerminalStreamRenderer = {
      create: createTerminalSurface,
      draw: (current) =>
        renderFence(
          scope.signal,
          () => scope.isCurrent() && element.isConnected,
          undefined,
          current,
        ),
    },
    display?: DisplayPreferences,
  ) {
    this.sink = new SurfaceSink(
      scope,
      element,
      palette,
      history,
      renderer,
      callbacks.observe,
      () => {
        this.check();
        this.ready = true;
        clearTimeout(this.timer);
      },
      callbacks.beforeDraw,
      display,
      callbacks.openLink,
    );
    this.consumer = new AttachmentV2Consumer(
      scope.generation,
      this.sink,
      async (generation, sequence) => {
        this.check();
        await callbacks.acknowledge(generation, sequence);
        this.check();
      },
    );
    scope.own(this);
    this.timer = setTimeout(
      () => this.cancel(new Error("terminal.bootstrap_timeout")),
      3000,
    );
  }
  private check(): void {
    if (this.disposed || !this.scope.isCurrent())
      throw new Error("view.retired");
    if (!this.ready && performance.now() - this.started >= 3000)
      throw new Error("terminal.bootstrap_timeout");
  }
  async enqueue(wire: ArrayBuffer | Uint8Array): Promise<V2Receipt | null> {
    try {
      this.check();
      const receipt = await this.consumer.enqueue(
        wire instanceof Uint8Array ? wire : new Uint8Array(wire),
      );
      this.check();
      return receipt;
    } catch (error) {
      if (this.paused) throw error;
      this.cancel(
        error instanceof Error ? error : new Error("terminal.consume_failed"),
      );
      throw error;
    }
  }
  focusOutput(): boolean {
    try {
      this.check();
      if (!this.ready || this.paused) return false;
      return this.sink.focusOutput();
    } catch {
      return false;
    }
  }
  geometry(): Readonly<Geometry> {
    this.check();
    return this.sink.geometry();
  }
  measuredDimensions(): Readonly<Geometry> | undefined {
    this.check();
    return this.sink.measuredDimensions();
  }
  /** Keep renderer/lease alive for native resize; cancel consumption/ACKs only. */
  pauseForResize(): void {
    this.check();
    if (!this.ready) throw new Error("fit.before_commit");
    this.pauseForLayout();
  }
  /** Layout only, not permission for input or a bootstrap ACK. */
  pauseForLayout(): void {
    this.check();
    this.paused = true;
    this.consumer.cancel(new Error("fit.paused"));
  }
  enableInput(
    listener: (text: string) => void,
    paste?: TerminalPasteOptions,
  ): () => void {
    this.check();
    if (!this.ready) throw new Error("input.before_commit");
    return this.sink.enableInput(listener, paste);
  }
  cancel(error = new Error("terminal.cancelled")): void {
    if (this.disposed || this.cancelling) return;
    this.cancelling = true;
    try {
      this.callbacks.failed?.(error);
    } finally {
      this.consumer.cancel(error);
      this.scope.release();
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    clearTimeout(this.timer);
    this.consumer.cancel(new Error("view.retired"));
    this.sink.dispose();
  }
}
