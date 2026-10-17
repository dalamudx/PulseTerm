// Fixed readonly validation surface. Separate queue/ACK path from primary session.
import type { Terminal as TerminalType } from "@xterm/headless";
import type { DomRenderer } from "../features/probe/dom-renderer";
import {
  parseQueryPaletteData,
  type QueryPaletteData,
  queryPaletteTheme,
} from "../features/terminal/query-palette";
import { suppressQueryReplies } from "../features/terminal/query-policy";
import { SharedUnicodeProvider } from "../features/terminal/unicode-provider";
import { Utf8Stream } from "../features/terminal/utf8-stream";
import {
  GenerationViewHost,
  type ViewScope,
} from "../features/terminal/view-host";
import { UiThemeOverrideTarget } from "../features/theme/ui-override-surface";
import { ViewThemePaintGuard } from "../features/theme/view-paint-guard";
import type { AttachBoundary } from "./attachment";
import { decodeFrame } from "./frames";
import {
  createProbeConsumer,
  type ProbeAttachmentVersion,
  type ProbeConsumer,
  probeAttachmentEnvelope,
} from "./probe-attachment-codec";
export type TransferTerminal = Pick<
  TerminalType,
  | "buffer"
  | "cols"
  | "rows"
  | "write"
  | "unicode"
  | "parser"
  | "onData"
  | "dispose"
>;
export class TransferViewProbe {
  private consumer: ProbeConsumer | undefined;
  private terminal: TransferTerminal | undefined;
  private renderer: DomRenderer | undefined;
  private boundary: AttachBoundary | undefined;
  private generation: bigint | undefined;
  private readonly lifetime = new AbortController();
  private readonly themeHost = new GenerationViewHost(
    this.lifetime.signal,
    (e) => this.fail(e),
  );
  private themeScope: ViewScope | undefined;
  private themeGuard: ViewThemePaintGuard | undefined;
  private themeTarget: UiThemeOverrideTarget | undefined;
  private readonly cleanup: (() => void)[] = [];
  private decoder = new Utf8Stream();
  private latest: Promise<unknown> = Promise.resolve();
  private error: unknown;
  private confirmed = false;
  private committing = false;
  private replies = 0;
  private capturedPalette: QueryPaletteData | undefined;
  private paletteChecked = false;
  constructor(
    private readonly options: {
      version: ProbeAttachmentVersion;
      host?: HTMLElement;
      signal: AbortSignal;
      ack: (generation: string, sequence: number) => Promise<void>;
      confirm: (
        generation: string,
        boundary: AttachBoundary,
        digest: string,
      ) => Promise<boolean> | Promise<void>;
      retainedReady?: (generation: string) => Promise<void>;
      /** Read-only metadata from this exact Begin generation, not a latest theme read. */
      palette?: (generation: string) => Promise<string | null>;
      /** Validation-only retained view; snapshots still reject all continuation frames. */
      live?: {
        ready: () => Promise<void>;
        marker: (
          generation: string,
          boundary: AttachBoundary,
          digest: string,
        ) => Promise<void>;
      };
      digest: (terminal: TransferTerminal) => string;
      failed: (error: unknown) => void;
    },
  ) {
    const abort = () => this.dispose();
    options.signal.addEventListener("abort", abort, { once: true });
    this.cleanup.push(() => options.signal.removeEventListener("abort", abort));
    if (options.signal.aborted) this.dispose();
  }
  receive(data: ArrayBuffer): void {
    try {
      if (this.lifetime.signal.aborted) throw new Error("transfer.cancelled");
      if (!(data instanceof ArrayBuffer))
        throw new Error("transfer.not_binary");
      const wire = new Uint8Array(data);
      const frame = decodeFrame(wire);
      if (frame.kind !== 8) throw new Error("transfer.frame_kind");
      const header = probeAttachmentEnvelope(
        frame.payload,
        this.options.version,
      );
      if (!this.consumer) {
        if (header.part !== 1 || frame.sequence !== 0)
          throw new Error("transfer.no_begin");
        this.generation = header.generation;
        this.consumer = createProbeConsumer(
          this.options.version,
          header.generation,
          {
            reset: async (boundary, signal) => {
              if (this.terminal) throw new Error("transfer.repeated_reset");
              this.boundary = boundary;
              const combined = AbortSignal.any([signal, this.lifetime.signal]);
              const encoded = await this.options.palette?.(
                header.generation.toString(),
              );
              if (combined.aborted) throw new Error("transfer.cancelled");
              if (encoded != null)
                this.capturedPalette = parseQueryPaletteData(encoded);
              const base = this.capturedPalette
                ? queryPaletteTheme(this.capturedPalette)
                : undefined;
              if (this.options.host) {
                const { createDomRenderer } = await import(
                  "../features/probe/dom-renderer"
                );
                if (combined.aborted) throw new Error("transfer.cancelled");
                this.renderer = createDomRenderer(
                  this.options.host,
                  boundary,
                  combined,
                  base,
                );
                this.terminal = this.renderer.terminal;
                if (this.capturedPalette) {
                  const { assertRenderedPalette } = await import(
                    "../features/probe/terminal-palette"
                  );
                  if (combined.aborted) throw new Error("transfer.cancelled");
                  assertRenderedPalette(
                    this.renderer.terminal,
                    this.capturedPalette,
                  );
                }
                this.themeScope = this.themeHost.begin(header.generation);
                this.cleanup.push(() => this.themeHost.dispose());
              } else {
                const { default: headless } = await import("@xterm/headless");
                if (combined.aborted) throw new Error("transfer.cancelled");
                this.terminal = new headless.Terminal({
                  cols: boundary.columns,
                  rows: boundary.rows,
                  scrollback: 128,
                  allowProposedApi: true,
                });
              }
              const provider = new SharedUnicodeProvider();
              this.terminal.unicode.register(provider);
              this.terminal.unicode.activeVersion = provider.version;
              const query = suppressQueryReplies(this.terminal);
              const input = this.terminal.onData(() => this.replies++);
              this.cleanup.push(query, () => input.dispose());
            },
            marker: async (_revision, _parsedBytes, signal) => {
              if (
                !this.options.live ||
                !this.confirmed ||
                signal.aborted ||
                this.lifetime.signal.aborted
              )
                throw new Error("transfer.marker_unsupported");
              await this.renderer?.settle(
                AbortSignal.any([signal, this.lifetime.signal]),
              );
              this.renderer?.evidence();
            },
            write: async (bytes, signal) => {
              if (
                !this.terminal ||
                signal.aborted ||
                this.lifetime.signal.aborted
              )
                throw new Error("transfer.cancelled");
              for (let at = 0; at < bytes.length; at += 4096) {
                const text = this.decoder.feed(bytes.subarray(at, at + 4096));
                if (!text) continue;
                await new Promise<void>((resolve, reject) => {
                  const abort = () => reject(new Error("transfer.cancelled"));
                  const combined = AbortSignal.any([
                    signal,
                    this.lifetime.signal,
                  ]);
                  combined.addEventListener("abort", abort, { once: true });
                  this.terminal?.write(text, () => {
                    combined.removeEventListener("abort", abort);
                    if (combined.aborted)
                      reject(new Error("transfer.cancelled"));
                    else resolve();
                  });
                });
              }
              await this.renderer?.settle(
                AbortSignal.any([signal, this.lifetime.signal]),
              );
              this.renderer?.evidence();
            },
          },
          async (g, s) => {
            if (this.lifetime.signal.aborted)
              throw new Error("transfer.cancelled");
            await this.options.ack(g.toString(), s);
          },
          8,
          8 * (6 + 28 + 8192),
        );
      }
      if (
        (this.confirmed &&
          !(this.options.live && (header.part === 5 || header.part === 8))) ||
        this.committing ||
        header.generation !== this.generation ||
        header.part < 1 ||
        (header.part > 4 &&
          !(
            this.options.live &&
            this.confirmed &&
            (header.part === 5 || header.part === 8)
          ))
      )
        throw new Error("transfer.unexpected_frame");
      if (header.part === 4) this.committing = true;
      const job = this.consumer.enqueue(wire).then(async (receipt) => {
        if (!receipt || receipt.event.kind === "staged") return;
        if (receipt.event.kind === "data") {
          if (
            !this.options.live ||
            !this.confirmed ||
            !this.terminal ||
            this.lifetime.signal.aborted
          )
            throw new Error("transfer.unexpected_data");
          return;
        }
        if (receipt.event.kind === "marker") {
          if (
            !this.options.live ||
            !this.confirmed ||
            !this.terminal ||
            !this.boundary ||
            this.generation === undefined ||
            this.lifetime.signal.aborted ||
            this.replies !== 0
          )
            throw new Error("transfer.invalid_marker");
          this.boundary = {
            ...this.boundary,
            revision: receipt.event.revision,
            parsedBytes: receipt.event.parsedBytes,
          };
          await this.options.live.marker(
            this.generation.toString(),
            this.boundary,
            this.options.digest(this.terminal),
          );
          if (this.lifetime.signal.aborted)
            throw new Error("transfer.cancelled");
          return;
        }
        if (
          receipt.event.kind !== "ready" ||
          !this.terminal ||
          !this.boundary ||
          this.generation === undefined ||
          this.lifetime.signal.aborted ||
          this.replies !== 0
        )
          throw new Error("transfer.invalid_receipt");
        const retainTheme = await this.options.confirm(
          this.generation.toString(),
          this.boundary,
          this.options.digest(this.terminal),
        );
        if (this.lifetime.signal.aborted) throw new Error("transfer.cancelled");
        this.confirmed = true;
        this.committing = false;
        if (this.options.live) {
          await this.options.live.ready();
          if (this.lifetime.signal.aborted)
            throw new Error("transfer.cancelled");
        } else if (retainTheme !== true) this.releaseSurface();
        else await this.options.retainedReady?.(this.generation.toString());
      });
      this.latest = job;
      void job.catch((error) => this.fail(error));
    } catch (error) {
      this.fail(error);
    }
  }
  private fail(error: unknown): void {
    if (this.error) return;
    this.error = error;
    this.dispose();
    this.options.failed(error);
  }
  /** Controlled readonly snapshot only. Actual browser color state, never a PTY query. */
  async verifyCapturedPalette(generation: string): Promise<void> {
    if (
      !this.confirmed ||
      this.options.live ||
      this.paletteChecked ||
      !this.renderer ||
      !this.capturedPalette ||
      generation !== this.generation?.toString() ||
      !this.themeScope?.isCurrent()
    ) {
      throw new Error("terminal.palette.view_retired");
    }
    const { verifyPaletteResets } = await import(
      "../features/probe/terminal-palette"
    );
    if (!this.themeScope.isCurrent())
      throw new Error("terminal.palette.view_retired");
    await verifyPaletteResets(
      this.renderer.terminal,
      this.capturedPalette,
      this.themeScope.signal,
    );
    if (!this.themeScope.isCurrent() || this.replies !== 0)
      throw new Error("terminal.palette.query_leak");
    this.paletteChecked = true;
  }
  /** Full stock Radix UI on the actual ordered Begin; no terminal palette change. */
  async paintTheme(
    generation: string,
    reader: (signal: AbortSignal) => Promise<unknown>,
  ): Promise<void> {
    const scope = this.themeScope,
      host = this.options.host;
    if (
      !scope ||
      !host ||
      !this.confirmed ||
      generation !== this.generation?.toString() ||
      !scope.isCurrent()
    )
      throw new Error("theme.delivery.view_retired");
    const { resolveUiTheme } = await import("../features/theme/radix-theme");
    if (!scope.isCurrent() || this.themeScope !== scope)
      throw new Error("theme.delivery.view_retired");
    this.themeTarget ??= new UiThemeOverrideTarget(scope, host);
    this.themeGuard ??= new ViewThemePaintGuard(scope);
    const target = this.themeTarget;
    if (
      (await this.themeGuard.refresh(reader, (data) =>
        target.prepare(resolveUiTheme(data, { contrast: "reject" }).paint),
      )) !== "applied" ||
      !scope.isCurrent()
    )
      throw new Error("theme.delivery.stale");
  }
  /** A content-free trusted notification can clear only this exact live Begin generation. */
  revokeTheme(generation: string): boolean {
    if (
      !this.confirmed ||
      generation !== this.generation?.toString() ||
      !this.themeScope?.isCurrent() ||
      !this.themeGuard
    )
      return false;
    this.themeGuard.revoke();
    return true;
  }
  private releaseSurface(): void {
    for (const stop of this.cleanup.splice(0)) stop();
    this.renderer?.dispose();
    if (!this.renderer) this.terminal?.dispose();
    this.terminal = undefined;
    this.renderer = undefined;
  }
  async finish(): Promise<void> {
    await this.latest;
    if (this.error) throw this.error;
    if (!this.confirmed) throw new Error("transfer.not_confirmed");
  }
  dispose(): void {
    this.lifetime.abort();
    this.consumer?.cancel();
    this.releaseSurface();
  }
}
