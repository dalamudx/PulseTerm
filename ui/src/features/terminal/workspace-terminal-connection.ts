/// <reference types="vite/client" />
import { Channel, invoke } from "@tauri-apps/api/core";
import { AttachmentV2Receiver } from "../../ipc/attachment-v2";
import {
  decodeProfileAppearance,
  type ProfileAppearance,
  profileDisplay,
} from "../../workspace-profile-editor";
import { isWorkspaceNonce } from "../../workspace-readiness";
import { readPreparedClipboard } from "./clipboard-read";
import type { DisplayPreferences } from "./display-preferences";
import { FitScheduler, type Geometry } from "./fit-scheduler";
import { InputQueue } from "./input-queue";
import { parseLayoutReceipt } from "./layout-receipt";
import { LoadStateProbe } from "./load-state-probe";
import { parseQueryPaletteData } from "./query-palette";
import type { TerminalPasteOptions } from "./terminal-surface";
import {
  type DrawnBoundary,
  type TerminalStreamRenderer,
  TerminalViewStream,
} from "./terminal-view-stream";
import type { GenerationViewHost } from "./view-host";

interface Prepared {
  ticket: string;
  generation: string;
  palette: string;
  history: number;
  ended: boolean;
  appearance?: ProfileAppearance;
}
export interface TerminalConnectionRpc {
  prepare(nonce: string, session: string): Promise<unknown>;
  connect(
    nonce: string,
    connection: string,
    channel: unknown,
  ): Promise<unknown>;
  acknowledge(
    nonce: string,
    connection: string,
    sequence: number,
  ): Promise<unknown>;
  confirm(
    nonce: string,
    connection: string,
    boundary: Readonly<DrawnBoundary>,
  ): Promise<unknown>;
  resize?(
    nonce: string,
    connection: string,
    size: Readonly<Geometry>,
  ): Promise<unknown>;
  inputReady?(nonce: string, connection: string): Promise<unknown>;
  enableInput?(nonce: string, connection: string): Promise<unknown>;
  prepareClipboardRead?(nonce: string, connection: string): Promise<unknown>;
  openLink?(nonce: string, url: string): Promise<unknown>;
  cancelClipboardRead?(nonce: string, grant: string): Promise<unknown>;
  readClipboardText?(nonce: string, grant: string): Promise<unknown>;
  input?(
    nonce: string,
    connection: string,
    bytes: Uint8Array,
  ): Promise<unknown>;
  detach(nonce: string, connection: string): Promise<unknown>;
}
const native: TerminalConnectionRpc = {
  openLink: (nonce, url) =>
    invoke("workspace_terminal_link_open", { nonce, target: { url } }),
  prepareClipboardRead: (nonce, connection) =>
    invoke("workspace_terminal_clipboard_prepare", { nonce, connection }),
  cancelClipboardRead: (nonce, grant) =>
    invoke("workspace_terminal_clipboard_cancel", { nonce, grant }),
  readClipboardText: (nonce, grant) =>
    invoke("workspace_terminal_clipboard_read", { nonce, grant }),
  resize: (nonce, connection, size) =>
    invoke("workspace_terminal_resize", {
      nonce,
      connection,
      columns: size.columns,
      rows: size.rows,
    }),
  inputReady: (nonce, connection) =>
    invoke("workspace_terminal_input_ready", { nonce, connection }),
  enableInput: (nonce, connection) =>
    invoke("workspace_terminal_enable_input", { nonce, connection }),
  input: (nonce, connection, bytes) =>
    invoke("workspace_terminal_input", {
      nonce,
      connection,
      bytes: Array.from(bytes),
    }),
  prepare: (nonce, session) =>
    invoke("workspace_terminal_prepare", { nonce, session }),
  connect: (nonce, connection, channel) =>
    invoke("workspace_terminal_connect", { nonce, connection, channel }),
  acknowledge: (nonce, connection, sequence) =>
    invoke("workspace_terminal_ack", { nonce, connection, sequence }),
  confirm: (nonce, connection, b) =>
    invoke("workspace_terminal_confirm", {
      nonce,
      connection,
      rendered: {
        columns: b.columns,
        rows: b.rows,
        revision: b.revision.toString(),
        sourceBytes: b.sourceBytes.toString(),
        utf8Bytes: b.utf8Bytes.toString(),
        digest: b.digest,
      },
    }),
  detach: (nonce, connection) =>
    invoke("workspace_terminal_detach", { nonce, connection }),
};
function counter(value: unknown): value is string {
  if (typeof value !== "string" || !/^[1-9][0-9]{0,19}$/.test(value))
    return false;
  return BigInt(value) <= (1n << 64n) - 1n;
}
function prepared(value: unknown): Readonly<Prepared> {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw new Error("terminal.prepare_invalid");
  const root = value as Record<string, unknown>;
  if (
    Object.keys(root).length !== (Object.hasOwn(root, "appearance") ? 6 : 5) ||
    !Object.keys(root).every((key) =>
      [
        "ticket",
        "generation",
        "palette",
        "history",
        "ended",
        "appearance",
      ].includes(key),
    ) ||
    typeof root.ticket !== "string" ||
    !isWorkspaceNonce(root.ticket) ||
    root.ticket !== root.ticket.toLowerCase() ||
    !counter(root.generation) ||
    typeof root.palette !== "string" ||
    typeof root.ended !== "boolean" ||
    typeof root.history !== "number" ||
    !Number.isInteger(root.history) ||
    root.history < 0 ||
    root.history > 10000
  )
    throw new Error("terminal.prepare_invalid");
  parseQueryPaletteData(root.palette);
  return Object.freeze({
    ticket: root.ticket,
    generation: root.generation,
    palette: root.palette,
    history: root.history,
    ended: root.ended,
    ...(Object.hasOwn(root, "appearance")
      ? { appearance: decodeProfileAppearance(root.appearance) }
      : {}),
  });
}
export interface TerminalConnectionOptions {
  nonce: string;
  session: string;
  element: HTMLElement;
  host: GenerationViewHost;
  signal: AbortSignal;
  failed(error: Error): void;
  input?: boolean;
  paste?: TerminalPasteOptions;
  display?: DisplayPreferences;
  /** Other observations are unconfirmed drawing data, never a backend freshness receipt. */
  observed?(boundary: Readonly<DrawnBoundary>): void;
  rpc?: TerminalConnectionRpc;
  channel?: (receive: (wire: ArrayBuffer) => void) => unknown;
  renderer?: TerminalStreamRenderer;
}
/** Existing root session only; never starts a shell or sends input. Palette is
 * ready before connect. Only a validated Raw Begin can create a ViewScope. */
/** Only a known native success (or a never-dispatched target reversal). */
export class TerminalLayoutChanged extends Error {
  constructor() {
    super("terminal.layout_changed");
  }
}
export class TerminalDetachFailed extends Error {
  constructor() {
    super("terminal.detach_failed");
  }
}
function detachTerminal(
  rpc: TerminalConnectionRpc,
  nonce: string,
  ticket: string,
  retirementConfirmed = false,
): Promise<void> {
  return new Promise<void>((resolve, reject) => {
    const timer = setTimeout(() => reject(new TerminalDetachFailed()), 1500);
    try {
      void rpc.detach(nonce, ticket).then(
        () => {
          clearTimeout(timer);
          resolve();
        },
        (error: unknown) => {
          clearTimeout(timer);
          if (retirementConfirmed && error === "workspace.denied") resolve();
          else reject(new TerminalDetachFailed());
        },
      );
    } catch {
      clearTimeout(timer);
      reject(new TerminalDetachFailed());
    }
  });
}
export class WorkspaceTerminalConnection {
  readonly ready: Promise<void>;
  private resolve!: () => void;
  private detached: Promise<void> | undefined;
  private reject!: (error: Error) => void;
  private stream: TerminalViewStream | undefined;
  private queue: InputQueue | undefined;
  private stopInput: (() => void) | undefined;
  private captured = false;
  private ended = false;
  private endedAcknowledged = false;
  private layoutConfirmed = false;
  private resizing = false;
  private announced = false;
  private geometryObserver: (() => void) | undefined;
  private bootstrapFit: FitScheduler | undefined;
  private bootstrapDispatched = false;
  private stopBootstrapObserver: (() => void) | undefined;
  private closed = false;
  private confirmed = false;
  private bootstrapped = false;
  private readonly started = performance.now();
  private timer: ReturnType<typeof setTimeout> | undefined;
  private readonly rpc: TerminalConnectionRpc;
  private loadProbe: LoadStateProbe | undefined;
  private readonly generation: bigint;
  private readonly linkNonce: string;
  private readonly aborted = () => this.dispose();
  constructor(
    private readonly options: TerminalConnectionOptions,
    private readonly capture: Readonly<Prepared>,
  ) {
    this.linkNonce = options.nonce;
    this.rpc = options.rpc ?? native;
    this.generation = BigInt(capture.generation);
    this.ready = new Promise((resolve, reject) => {
      this.resolve = resolve;
      this.reject = reject;
    });
    // Keep early failures handled even if the caller hasn't yet received this object.
    void this.ready.catch(() => {});
    this.timer = setTimeout(() => {
      if (!this.bootstrapDispatched) this.resizing = false;
      this.fail(new Error("terminal.bootstrap_timeout"));
    }, 3000);
    options.signal.addEventListener("abort", this.aborted, { once: true });
    if (options.signal.aborted) this.dispose();
  }
  async connect(): Promise<void> {
    if (this.closed) throw new Error("view.retired");
    const channel = (
      this.options.channel ?? ((receive) => new Channel<ArrayBuffer>(receive))
    )((wire) => this.receive(wire));
    try {
      await this.rpc.connect(this.options.nonce, this.capture.ticket, channel);
      if (this.closed) throw new Error("view.retired");
    } catch (error) {
      this.fail(error);
      throw error;
    }
  }
  private receive(wire: ArrayBuffer): void {
    if (this.closed || this.resizing || this.options.signal.aborted) return;
    try {
      this.checkDeadline();
      if (!(wire instanceof ArrayBuffer))
        throw new Error("terminal.non_raw_channel");
      if (!this.stream) {
        // Validate the complete first Begin, including size, kind, version,
        // sequence and both coordinates; no metadata-only local generation.
        const begin = new AttachmentV2Receiver(this.generation).accept(
          new Uint8Array(wire),
        );
        if (begin?.event.kind !== "staged")
          throw new Error("terminal.invalid_begin");
        const scope = this.options.host.begin(this.generation);
        scope.own(() => this.fail(new Error("view.retired")));
        if (import.meta.env.VITE_LOAD_STATE_PROBE === "1") {
          this.loadProbe = new LoadStateProbe({
            element: this.options.element,
            signal: scope.signal,
            isCurrent: () =>
              scope.isCurrent() &&
              this.bootstrapped &&
              !this.closed &&
              !this.resizing,
            nonce: this.options.nonce,
            connection: this.capture.ticket,
            failed: (error) => this.fail(error),
          });
          scope.own(this.loadProbe);
        }
        this.stream = new TerminalViewStream(
          scope,
          this.options.element,
          this.capture.palette,
          this.capture.history,
          {
            openLink: async (url) => {
              if (
                this.closed ||
                this.resizing ||
                !this.bootstrapped ||
                this.options.signal.aborted
              )
                throw new Error("view.retired");
              if (!this.rpc.openLink)
                throw new Error("workspace.link_unavailable");
              await this.rpc.openLink(this.linkNonce, url);
            },
            failed: (error) => this.fail(error),
            beforeDraw: () => this.beforeDraw(),
            acknowledge: async (generation, sequence) => {
              if (this.closed || generation !== this.generation)
                throw new Error("view.retired");
              const ended = this.ended;
              try {
                await this.rpc.acknowledge(
                  this.options.nonce,
                  this.capture.ticket,
                  sequence,
                );
                if (ended) this.endedAcknowledged = true;
              } catch (error) {
                this.fail(error);
                throw error;
              }
            },
            observe: async (boundary, signal) => {
              if (this.closed || signal.aborted)
                throw new Error("view.retired");
              if (boundary.kind === "capture") {
                try {
                  await this.rpc.confirm(
                    this.options.nonce,
                    this.capture.ticket,
                    boundary,
                  );
                } catch (error) {
                  this.fail(error);
                  throw error;
                }
                if (this.closed || signal.aborted)
                  throw new Error("view.retired");
                this.checkDeadline();
                this.captured = true;
              }
              if (boundary.kind === "end") {
                this.ended = true;
                this.queue?.cancel();
                this.stopInput?.();
                if (this.captured) {
                  this.confirmed = true;
                  this.bootstrapped = true;
                  clearTimeout(this.timer);
                  this.resolve();
                }
              }
              await this.loadProbe?.observe(boundary);
              this.options.observed?.(boundary);
            },
          },
          this.options.renderer,
          this.capture.appearance
            ? profileDisplay(this.options.display, this.capture.appearance)
            : this.options.display,
        );
      }
      void this.stream
        .enqueue(wire)
        .then(async (receipt) => {
          if (receipt?.event.kind !== "ready" || this.resizing) return;
          if (this.ended) {
            this.confirmed = true;
            this.bootstrapped = true;
            clearTimeout(this.timer);
            this.resolve();
            return;
          }
          if (!this.captured || this.closed) throw new Error("view.retired");
          if (this.capture.ended) return; // Readonly final capture awaits its ordered End, never input enable.
          if (this.options.input) {
            if (!this.rpc.enableInput || !this.rpc.input)
              throw new Error("input.unsupported");
            try {
              await this.rpc.enableInput(
                this.options.nonce,
                this.capture.ticket,
              );
            } catch (error) {
              if (this.ended) return;
              throw error;
            }
            if (this.resizing) return;
            if (this.ended) {
              this.confirmed = true;
              this.bootstrapped = true;
              clearTimeout(this.timer);
              this.resolve();
              return;
            }
            if (this.closed) throw new Error("view.retired");
            this.checkDeadline();
            this.confirmed = true;
            this.queue = new InputQueue(async (bytes) => {
              if (
                this.closed ||
                this.ended ||
                this.resizing ||
                !this.confirmed ||
                !this.rpc.input
              )
                throw new Error("view.retired");
              await this.rpc.input(
                this.options.nonce,
                this.capture.ticket,
                bytes,
              );
            }, this.options.signal);
            const prepareClipboardRead = this.rpc.prepareClipboardRead?.bind(
              this.rpc,
            );
            const readClipboardText = this.rpc.readClipboardText?.bind(
              this.rpc,
            );
            const cancelClipboardRead = this.rpc.cancelClipboardRead?.bind(
              this.rpc,
            );
            this.stopInput = this.stream?.enableInput(
              (text) => {
                void this.queue?.enqueue(text).catch((error: unknown) => {
                  if (!this.ended) this.fail(error);
                });
              },
              this.options.paste && {
                ...this.options.paste,
                readClipboardText:
                  readClipboardText &&
                  prepareClipboardRead &&
                  cancelClipboardRead
                    ? (signal) =>
                        readPreparedClipboard(
                          signal,
                          () =>
                            !this.closed &&
                            !this.ended &&
                            !this.options.signal.aborted,
                          {
                            prepare: () =>
                              prepareClipboardRead(
                                this.options.nonce,
                                this.capture.ticket,
                              ),
                            read: (grant) =>
                              readClipboardText(this.options.nonce, grant),
                            cancel: (grant) =>
                              cancelClipboardRead(this.options.nonce, grant),
                          },
                        )
                    : undefined,
              },
            );

            if (this.closed) throw new Error("view.retired");
          }
          this.checkDeadline();
          this.confirmed = true;
          clearTimeout(this.timer);
          this.bootstrapped = true;
          this.resolve();
        })
        .catch((error: unknown) => this.fail(error));
    } catch (error) {
      this.fail(error);
    }
  }
  private layoutChanged(): void {
    if (this.closed || this.options.signal.aborted) return;
    this.reject(new TerminalLayoutChanged());
    this.dispose();
  }
  private beforeDraw(): void {
    if (this.closed) return;
    if (this.capture.ended) return; // No geometry/control authority for a stopped actor.
    if (this.bootstrapped) {
      this.geometryObserver?.();
      return;
    }
    if (this.resizing) return;
    const current = this.stream?.geometry(),
      size = this.stream?.measuredDimensions();
    if (
      !current ||
      !size ||
      (current.columns === size.columns && current.rows === size.rows)
    )
      return;
    this.resizing = true;
    this.stream?.pauseForLayout();
    const measure = () => {
      if (this.closed || this.options.signal.aborted) return;
      const latest = this.stream?.measuredDimensions();
      if (
        latest &&
        !this.bootstrapDispatched &&
        latest.columns === current.columns &&
        latest.rows === current.rows
      ) {
        this.layoutChanged();
        return;
      }
      this.bootstrapFit?.observe(latest);
    };
    this.bootstrapFit = new FitScheduler(
      current,
      async (target) => {
        this.bootstrapDispatched = true;
        clearTimeout(this.timer);
        await this.resize(target);
      },
      () => this.layoutChanged(),
      (error) => {
        if (this.closed || this.options.signal.aborted) return;
        this.resizing = false;
        this.fail(error);
      },
      this.options.signal,
    );
    if (typeof ResizeObserver !== "undefined") {
      const observer = new ResizeObserver(measure);
      observer.observe(this.options.element);
      this.stopBootstrapObserver = () => observer.disconnect();
    }
    measure();
  }
  private checkDeadline(): void {
    if (!this.bootstrapped && performance.now() - this.started >= 3000)
      throw new Error("terminal.bootstrap_timeout");
  }
  private fail(error: unknown): void {
    if (this.closed || this.resizing) return;
    if (this.options.signal.aborted) {
      this.dispose();
      return;
    }
    const failure =
      error instanceof Error ? error : new Error("terminal.connection_failed");
    this.reject(failure);
    this.dispose();
    try {
      this.options.failed(failure);
    } catch {
      /* Do not hide cleanup. */
    }
  }
  watchGeometry(listener: () => void): () => void {
    if (this.closed) throw new Error("view.retired");
    this.geometryObserver = listener;
    return () => {
      if (this.geometryObserver === listener) this.geometryObserver = undefined;
    };
  }
  async announceInputReady(): Promise<void> {
    if (this.closed || this.ended || this.resizing || !this.confirmed)
      throw new Error("fit.not_ready");
    if (this.announced) return;
    this.announced = true;
    await this.rpc.inputReady?.(this.options.nonce, this.capture.ticket);
    if (this.closed || this.options.signal.aborted)
      throw new Error("view.retired");
  }
  /** Confirmed display focus only; ended output stays read-only. No RPC or credit change. */
  focusOutput(): boolean {
    if (this.closed || this.resizing || !this.confirmed || !this.captured)
      return false;
    return this.stream?.focusOutput() ?? false;
  }
  hasEnded(): boolean {
    return this.ended;
  }
  geometry(): Readonly<Geometry> {
    if (this.closed || !this.confirmed || !this.stream)
      throw new Error("fit.not_ready");
    return this.stream.geometry();
  }
  measuredDimensions(): Readonly<Geometry> | undefined {
    if (this.closed || !this.confirmed) return;
    return this.stream?.measuredDimensions();
  }
  pauseForResize(): void {
    if (this.closed || this.ended || !this.confirmed || !this.stream)
      throw new Error("fit.not_ready");
    if (this.resizing) return;
    this.resizing = true;
    this.stopInput?.();
    this.queue?.cancel();
    this.stream.pauseForResize();
  }
  async resize(size: Readonly<Geometry>): Promise<void> {
    if (this.closed || this.ended || !this.resizing || !this.rpc.resize)
      throw new Error("fit.not_ready");
    const receipt = await this.rpc.resize(
      this.options.nonce,
      this.capture.ticket,
      size,
    );
    if (this.closed || this.options.signal.aborted)
      throw new Error("view.retired");
    parseLayoutReceipt(receipt, size);
    // A validated native layout result may retire the old forwarding ticket.
    // A denial for that ticket then means cleanup already completed; no input
    // or reattachment is admitted without a new backend-authorized binding.
    this.layoutConfirmed = true;
  }
  /** Stop local work synchronously; reattachment must await this exact receipt. */
  suspend(): Promise<void> {
    if (this.detached) return this.detached;
    this.closed = true;
    this.loadProbe?.dispose();
    this.detached = detachTerminal(
      this.rpc,
      this.options.nonce,
      this.capture.ticket,
      this.endedAcknowledged || this.layoutConfirmed,
    );
    void this.detached.catch(() => {});
    this.geometryObserver = undefined;
    this.bootstrapFit?.dispose();
    this.stopBootstrapObserver?.();
    clearTimeout(this.timer);
    this.options.signal.removeEventListener("abort", this.aborted);
    this.reject(new Error("view.retired"));
    this.queue?.cancel();
    this.stopInput?.();
    this.stream?.cancel();
    return this.detached;
  }
  dispose(): void {
    // Page retirement can reject the receipt; never close the root session or retry.
    void this.suspend().catch(() => {});
  }
}
export async function prepareWorkspaceTerminal(
  options: TerminalConnectionOptions,
): Promise<WorkspaceTerminalConnection> {
  if (
    options.signal.aborted ||
    !isWorkspaceNonce(options.nonce) ||
    !counter(options.session)
  )
    throw new Error("workspace.denied");
  const rpc = options.rpc ?? native;
  const value = await rpc.prepare(options.nonce, options.session);
  // Even malformed drawing metadata must release a valid server-issued ticket.
  let capture: Readonly<Prepared>;
  try {
    capture = prepared(value);
  } catch (error) {
    if (
      value &&
      typeof value === "object" &&
      "ticket" in value &&
      typeof value.ticket === "string" &&
      isWorkspaceNonce(value.ticket)
    )
      await detachTerminal(rpc, options.nonce, value.ticket);
    throw error;
  }
  const connection = new WorkspaceTerminalConnection(options, capture);
  try {
    await connection.connect();
    return connection;
  } catch (error) {
    await connection.suspend();
    throw error;
  }
}
