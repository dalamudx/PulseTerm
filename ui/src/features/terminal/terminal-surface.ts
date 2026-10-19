import type {
  ITerminalInitOnlyOptions,
  ITerminalOptions,
  Terminal,
} from "@xterm/xterm";
import { CompositionGuard } from "../../keybindings/composition";
import { readSystemClipboard } from "./clipboard-read";
import { projectedRowText } from "./dom-projection";
import { type Geometry, measuredGrid } from "./fit-scheduler";
import { nonceDocument } from "./nonce-document";
import { type PasteAssessment, PasteGate } from "./paste-policy";
import { bindRightClickPaste } from "./right-click-paste";
import { bindAutoCopy } from "./selection-copy";
import { createTerminalBellNotice } from "./terminal-bell";
import {
  bindTerminalDeviceSpacing,
  terminalDeviceSpacing,
} from "./terminal-device-spacing";
import {
  terminalFontFamily,
  waitForBundledTerminalFont,
} from "./terminal-fonts";
import { TerminalImeCommitGuard } from "./terminal-ime-commit";
import { createScrollReturn } from "./terminal-scroll-return";
export interface TerminalPasteOptions {
  confirm(
    text: string,
    assessment: PasteAssessment,
    signal: AbortSignal,
  ): Promise<boolean>;
  rejected?(code: string): void;
  submitted?(): void;
  readClipboardText?(signal: AbortSignal): Promise<string>;
}

import { parseQueryPaletteData, queryPaletteTheme } from "./query-palette";
import { suppressQueryReplies } from "./query-policy";
import { createLinkReview } from "./terminal-link-review";
import { textLinkProvider } from "./terminal-links";
import { SharedUnicodeProvider } from "./unicode-provider";
import { Utf8Stream } from "./utf8-stream";
import type { ViewResource, ViewScope } from "./view-host";
import { visibleTerminalDigest } from "./visible-digest";
import {
  type LigatureTerminal,
  type WebglLoader,
  WebglRenderEvidence,
} from "./webgl-render-evidence";

export type SurfaceDriver = Pick<
  Terminal,
  | "options"
  | "focus"
  | "open"
  | "write"
  | "dispose"
  | "parser"
  | "unicode"
  | "onData"
  | "paste"
  | "hasSelection"
  | "getSelection"
  | "attachCustomKeyEventHandler"
  | "buffer"
  | "cols"
  | "rows"
  | "element"
  | "refresh"
> &
  Partial<
    Pick<
      Terminal,
      | "modes"
      | "loadAddon"
      | "onRender"
      | "registerCharacterJoiner"
      | "deregisterCharacterJoiner"
      | "scrollToBottom"
      | "onBell"
      | "onScroll"
      | "registerLinkProvider"
      | "input"
    >
  >;
export type SurfaceFactory = (
  options: ITerminalOptions & ITerminalInitOnlyOptions,
) => SurfaceDriver;

import {
  type DisplayPreferences,
  displayPreferences,
} from "./display-preferences";
export interface SurfaceGeometry {
  readonly columns: number;
  readonly rows: number;
  readonly history: number;
  readonly display?: DisplayPreferences;
  readonly openLink?: (url: string) => Promise<void>;
}
async function loadTerminal(): Promise<SurfaceFactory> {
  const [{ Terminal }] = await Promise.all([
    import("@xterm/xterm"),
    import("@xterm/xterm/css/xterm.css"),
  ]);
  return (options) => new Terminal(options);
}
function current(scope: ViewScope): void {
  if (!scope.isCurrent()) throw new Error("view.retired");
}
/** Trusted renderer surface only: no session ownership or IPC permission. The
 * document MUST carry its runtime Tauri style nonce. No global document patch. */
export async function createTerminalSurface(
  scope: ViewScope,
  element: HTMLElement,
  encodedPalette: string,
  geometry: SurfaceGeometry,
  load: () => Promise<SurfaceFactory> = loadTerminal,
  loadWebgl?: WebglLoader,
): Promise<TerminalSurface> {
  return scope.create(async () => {
    current(scope);
    const { columns, rows, history } = geometry;
    if (
      !Number.isInteger(columns) ||
      columns < 1 ||
      columns > 512 ||
      !Number.isInteger(rows) ||
      rows < 1 ||
      rows > 256 ||
      !Number.isInteger(history) ||
      history < 0 ||
      history > 10000
    )
      throw new Error("terminal.geometry_invalid");
    const display = displayPreferences(
      geometry.display ?? {
        fontSize: 13,
        fontFamily: "monospace",
        cursorBlink: false,
      },
    );
    const theme = queryPaletteTheme(parseQueryPaletteData(encodedPalette));
    const documentOverride = nonceDocument(element.ownerDocument);
    await waitForBundledTerminalFont(
      element.ownerDocument,
      display,
      scope.signal,
    );
    current(scope);
    const factory = await load();
    current(scope);
    const terminal = factory({
      cols: columns,
      rows,
      scrollback: history,
      theme,
      documentOverride,
      allowProposedApi: true,
      convertEol: false,
      disableStdin: true,
      fontFamily: terminalFontFamily(display),
      fontSize: display.fontSize,
      lineHeight: (display.lineHeightPercent ?? 100) / 100,
      letterSpacing: terminalDeviceSpacing(
        display.letterSpacing ?? 0,
        element.ownerDocument,
      ),
      cursorBlink: display.cursorBlink,
      macOptionIsMeta: display.macOptionIsMeta ?? false,
      scrollOnUserInput: display.scrollOnInput ?? true,
      linkHandler: {
        activate: (event) => {
          event.preventDefault();
        },
        allowNonHttpProtocols: false,
      },
    });
    let surface: TerminalSurface | undefined;
    try {
      current(scope);
      if (
        display.scrollOnOutput === true &&
        typeof terminal.scrollToBottom !== "function"
      )
        throw new Error("terminal.scroll_unavailable");
      const provider = new SharedUnicodeProvider();
      terminal.unicode.register(provider);
      terminal.unicode.activeVersion = provider.version;
      surface = new TerminalSurface(
        scope,
        terminal,
        suppressQueryReplies(terminal),
        display.rightClickPaste ?? false,
        display.scrollOnOutput ?? false,
        display.backspaceSend ?? "del",
        display.deleteSend ?? "csi",
      );
      terminal.open(element);
      current(scope);
      surface.bindDeviceSpacing(
        element.ownerDocument,
        display.letterSpacing ?? 0,
      );
      if (display.fontLigatures === true)
        await surface.enableLigatures(loadWebgl);
      surface.bindSelectionCopy(display.copyOnSelect ?? false);
      surface.bindBell(element);
      surface.bindScrollReturn(element);
      surface.bindLinks(element, geometry?.openLink);
      return surface;
    } catch (error) {
      if (surface) surface.dispose();
      else terminal.dispose();
      throw error;
    }
  });
}
/** Parsing completion is NOT a paint receipt. The ordered consumer must separately
 * perform its real-DOM draw gate before releasing frame credit. */
export class TerminalSurface implements ViewResource {
  private webgl: WebglRenderEvidence | undefined;
  private disposed = false;
  private writing = false;
  private scrollToken: object = {};
  private requestedViewport:
    | Readonly<{
        buffer: SurfaceDriver["buffer"]["active"];
        viewportY: number;
        baseY: number;
        columns: number;
        rows: number;
        scrollToken: object;
      }>
    | undefined;
  private decoder = new Utf8Stream();
  private bellSignal: AbortSignal | undefined;
  private refreshScrollReturn: (() => void) | undefined;
  private keyInputEnabled = false;
  private readonly lifetime = new AbortController();
  private readonly cleanup: (() => void)[];
  constructor(
    private readonly scope: ViewScope,
    private readonly terminal: SurfaceDriver,
    queryCleanup: () => void,
    private readonly rightClickPaste = false,
    private readonly scrollOnOutput = false,
    private readonly backspaceSend: "del" | "bs" = "del",
    private readonly deleteSend: "csi" | "del" = "csi",
  ) {
    this.cleanup = [queryCleanup];
  }
  bindDeviceSpacing(document: Document, cssPixels: number): void {
    this.cleanup.push(
      bindTerminalDeviceSpacing(
        this.terminal,
        document,
        cssPixels,
        this.lifetime.signal,
        () => !this.disposed && this.scope.isCurrent(),
      ),
    );
  }
  bindLinks(
    element: HTMLElement,
    openExternal?: (url: string) => Promise<void>,
  ): void {
    const current = () => !this.disposed && this.scope.isCurrent();
    const review = createLinkReview(
      element,
      this.lifetime.signal,
      current,
      () => {
        this.focusOutput();
      },
      element.ownerDocument,
      openExternal,
    );
    const activate = (event: MouseEvent, uri: string, source?: "file") => {
      event.preventDefault();
      if (current() && event.isTrusted && event.button === 0)
        review.open(uri, source === "file");
    };
    this.terminal.options.linkHandler = {
      activate: (event, uri) => activate(event, uri),
      allowNonHttpProtocols: false,
    };
    this.cleanup.push(() => review.dispose());
    if (typeof this.terminal.registerLinkProvider === "function") {
      const provider = this.terminal.registerLinkProvider(
        textLinkProvider(this.terminal, current, activate),
      );
      this.cleanup.push(() => provider.dispose());
    }
  }
  bindScrollReturn(element: HTMLElement): void {
    const terminal = this.terminal;
    if (
      typeof terminal.onScroll !== "function" ||
      typeof terminal.scrollToBottom !== "function" ||
      !terminal.buffer
    )
      return;
    const notice = createScrollReturn(
      element,
      () => !this.disposed && this.scope.isCurrent(),
      () => terminal.buffer.active,
      () => {
        terminal.scrollToBottom?.();
      },
      () => {
        this.focusOutput();
      },
    );
    this.refreshScrollReturn = () => notice.refresh();
    this.cleanup.push(() => {
      this.refreshScrollReturn = undefined;
      notice.dispose();
    });
    const listener = terminal.onScroll(() => {
      if (!this.disposed && this.scope.isCurrent()) {
        this.scrollToken = {};
        notice.refresh();
      }
    });
    this.cleanup.push(() => listener.dispose());
    notice.refresh();
  }
  bindBell(element: HTMLElement): void {
    if (typeof this.terminal.onBell !== "function") return;
    const notice = createTerminalBellNotice(
      element,
      () => !this.disposed && this.scope.isCurrent(),
      () => {
        this.focusOutput();
      },
    );
    this.cleanup.push(() => notice.dispose());
    const listener = this.terminal.onBell(() => {
      if (
        this.writing &&
        this.bellSignal &&
        !this.bellSignal.aborted &&
        !this.disposed &&
        this.scope.isCurrent()
      )
        notice.ring();
    });
    this.cleanup.push(() => listener.dispose());
  }
  async enableLigatures(load?: WebglLoader): Promise<void> {
    this.check();
    if (this.webgl) throw new Error("terminal.webgl_already_enabled");
    const renderer = await WebglRenderEvidence.create(
      this.scope,
      this.terminal as LigatureTerminal,
      load,
    );
    try {
      this.check();
      this.webgl = renderer;
      this.cleanup.push(() => renderer.dispose());
    } catch (error) {
      renderer.dispose();
      throw error;
    }
  }
  private check(): void {
    current(this.scope);
    if (this.disposed) throw new Error("terminal.surface_disposed");
  }
  /** Bounded, single-flight immutable copy; cancellation never submits a receipt. */
  writeParsed(bytes: Uint8Array, bellSignal?: AbortSignal): Promise<void> {
    this.check();
    if (bytes.byteLength > 65536) throw new Error("terminal.write_oversize");
    if (this.writing) throw new Error("terminal.write_busy");
    return this.writeText(this.decoder.feed(bytes.slice()), bellSignal);
  }
  /** Genuine ordered EOF only, never called for hiding, cancellation or repair. */
  finishParsed(): Promise<void> {
    this.check();
    if (this.writing) throw new Error("terminal.write_busy");
    return this.writeText(this.decoder.finish());
  }
  /** Discard only UTF-8 fragment state. The authorized screen replay MUST contain
   * CAN to reset the VT parser; no RIS, terminal reset or scrollback erase here. */
  beginRepair(): void {
    this.check();
    if (this.writing) throw new Error("terminal.write_busy");
    this.decoder = new Utf8Stream();
  }
  private writeText(text: string, bellSignal?: AbortSignal): Promise<void> {
    if (text.length === 0) return Promise.resolve();
    this.writing = true;
    this.bellSignal = bellSignal;
    return new Promise((resolve, reject) => {
      let done = false;
      const finish = (error?: Error) => {
        if (done) return;
        done = true;
        this.writing = false;
        this.bellSignal = undefined;
        this.scope.signal.removeEventListener("abort", abort);
        this.lifetime.signal.removeEventListener("abort", abort);
        if (error) reject(error);
        else resolve();
      };
      const abort = () => finish(new Error("view.retired"));
      this.scope.signal.addEventListener("abort", abort, { once: true });
      this.lifetime.signal.addEventListener("abort", abort, { once: true });
      try {
        this.check();
        this.terminal.write(text, () => {
          if (done) return;
          try {
            this.check();
          } catch {
            finish(new Error("view.retired"));
            return;
          }
          try {
            if (this.scrollOnOutput) this.terminal.scrollToBottom?.();
            this.refreshScrollReturn?.();
            finish();
          } catch {
            finish(new Error("terminal.scroll_failed"));
          }
        });
      } catch {
        finish(new Error("terminal.write_failed"));
      }
    });
  }
  /** Schedule only this renderer's rows; does not touch parser/history/palette. */
  prepareDraw(): void {
    this.check();
    if (this.writing) throw new Error("terminal.write_busy");
    this.webgl?.prepare();
    const buffer = this.terminal.buffer.active;
    this.requestedViewport = {
      buffer,
      viewportY: buffer.viewportY,
      baseY: buffer.baseY,
      columns: this.terminal.cols,
      rows: this.terminal.rows,
      scrollToken: this.scrollToken,
    };
    this.terminal.refresh(0, this.terminal.rows - 1);
  }
  /** A scroll can invalidate a pending paint even when it returns to the same row. */
  drawIsCurrent(): boolean {
    this.check();
    const requested = this.requestedViewport;
    const buffer = this.terminal.buffer.active;
    return (
      !!requested &&
      requested.buffer === buffer &&
      requested.viewportY === buffer.viewportY &&
      requested.baseY === buffer.baseY &&
      requested.columns === this.terminal.cols &&
      requested.rows === this.terminal.rows &&
      requested.scrollToken === this.scrollToken
    );
  }
  /** Called AFTER the two-rAF fence; DOM text/layout or completed GPU drawing
   * evidence. Neither is a desktop compositor acknowledgement. */
  verifyDraw(): void | Promise<void> {
    this.check();
    const root = this.terminal.element,
      screen = root?.querySelector<HTMLElement>(".xterm-screen");
    const rect = screen?.getBoundingClientRect(),
      window = root?.ownerDocument.defaultView;
    if (
      !root?.isConnected ||
      !rect ||
      !window ||
      rect.width <= 0 ||
      rect.height <= 0 ||
      rect.top < 0 ||
      rect.left < 0 ||
      rect.right > window.innerWidth ||
      rect.bottom > window.innerHeight
    )
      throw new Error("terminal.dom_geometry");
    if (this.webgl) return this.webgl.verify();
    const rows = root.querySelector(".xterm-rows")?.children,
      buffer = this.terminal.buffer.active;
    if (!rows || rows.length !== this.terminal.rows)
      throw new Error("terminal.dom_rows");
    for (let row = 0; row < this.terminal.rows; row++) {
      const line = buffer.getLine(buffer.viewportY + row);
      const actual = projectedRowText(rows[row]?.textContent ?? "");
      const expected = line
        ? projectedRowText(line.translateToString(true))
        : undefined;
      if (actual !== expected) throw new Error("terminal.dom_projection");
    }
  }
  /** User-selected displayed text only, including an ended read-only screen.
   * This is not a backend deep-history/search/export endpoint. OSC52 stays blocked. */
  bindSelectionCopy(copyOnSelect = false): void {
    this.check();
    const root = this.terminal.element;
    const copy = (event: Event) => {
      if (
        !event.isTrusted ||
        this.disposed ||
        !this.scope.isCurrent() ||
        !this.terminal.hasSelection()
      )
        return;
      event.preventDefault();
      event.stopImmediatePropagation();
      const clipboard = (event as ClipboardEvent).clipboardData;
      if (!clipboard) return;
      const text = this.terminal.getSelection();
      if (
        text.length > 4194304 ||
        new TextEncoder().encode(text).length > 4194304
      )
        return;
      clipboard.setData("text/plain", text);
    };
    const composition = root
      ? new CompositionGuard(root, this.lifetime.signal)
      : undefined;
    if (root) {
      const imeCommit = new TerminalImeCommitGuard(
        root,
        this.lifetime.signal,
        () => this.scope.isCurrent(),
      );
      this.cleanup.push(imeCommit.dispose);
    }
    root?.addEventListener("copy", copy, { capture: true });
    this.terminal.attachCustomKeyEventHandler((event) => {
      if (this.disposed || !this.scope.isCurrent()) return false;
      if (composition?.handles(event)) return true;
      if (
        ((event.key === "Backspace" && this.backspaceSend === "bs") ||
          (event.key === "Delete" && this.deleteSend === "del")) &&
        this.keyInputEnabled &&
        this.terminal.options.disableStdin === false &&
        this.terminal.input &&
        event.type === "keydown" &&
        event.isTrusted &&
        !event.defaultPrevented &&
        !event.ctrlKey &&
        !event.altKey &&
        !event.metaKey &&
        !event.shiftKey
      ) {
        event.preventDefault();
        this.terminal.input(event.key === "Delete" ? "\x7f" : "\b", true);
        return false;
      }
      if (
        event.type === "keydown" &&
        (event.ctrlKey || event.metaKey) &&
        !event.altKey &&
        event.key.toLowerCase() === "c" &&
        this.terminal.hasSelection()
      )
        return false;
      return true; // No selection: xterm sends the normal interrupt through the authorized input queue.
    });
    this.cleanup.push(() =>
      root?.removeEventListener("copy", copy, { capture: true }),
    );
    this.cleanup.push(
      bindAutoCopy(root, {
        enabled: copyOnSelect,
        current: () => !this.disposed && this.scope.isCurrent(),
        selected: () => this.terminal.hasSelection(),
        copy: () => {
          root?.ownerDocument.execCommand("copy");
        },
      }),
    );
  }
  /** Pure visible checksum; never feed VT, flush the decoder or grant permission. */
  visibleDigest(): string {
    this.check();
    if (this.writing) throw new Error("terminal.write_busy");
    return visibleTerminalDigest(this.terminal);
  }
  /** Arm only after native capture and supervised Commit ACK. No policy means block. */
  enableInput(
    listener: (text: string) => void,
    policy?: TerminalPasteOptions,
  ): () => void {
    this.check();
    const root = this.terminal.element;
    let closed = false;
    const report = (code: string) => {
      if (!closed) {
        try {
          policy?.rejected?.(code);
        } catch {
          /* Feedback never grants authority. */
        }
      }
    };
    const gate = policy
      ? new PasteGate(
          this.scope.signal,
          () => !closed && !this.disposed && this.scope.isCurrent(),
          policy.confirm,
          (text) => {
            this.check();
            this.terminal.paste(text);
          },
        )
      : undefined;
    const submit = async (text: string) => {
      if (!gate) return;
      try {
        const result = await gate.submit(text);
        if (result !== "submitted") report(`paste.${result}`);
        else if (!closed && !this.disposed && this.scope.isCurrent())
          policy?.submitted?.();
      } catch (error) {
        const code =
          error instanceof Error &&
          [
            "paste.size",
            "paste.invalid_unicode",
            "paste.unsupported_control",
          ].includes(error.message)
            ? error.message
            : "paste.unavailable";
        report(code);
      }
    };
    const paste = (event: Event) => {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!gate || !event.isTrusted) return;
      try {
        const text = (event as ClipboardEvent).clipboardData?.getData(
          "text/plain",
        );
        if (text) void submit(text);
      } catch {
        report("paste.unavailable");
      }
    };
    const stopRightClick =
      root && gate && this.rightClickPaste
        ? bindRightClickPaste(root, {
            signal: this.scope.signal,
            current: () => !closed && !this.disposed && this.scope.isCurrent(),
            enabled: () => this.rightClickPaste,
            mouseMode: () => this.terminal.modes?.mouseTrackingMode,
            focus: () => this.terminal.focus(),
            readText: (signal) =>
              policy?.readClipboardText
                ? policy.readClipboardText(signal)
                : readSystemClipboard(root.ownerDocument, signal, () =>
                    this.scope.isCurrent(),
                  ),
            submit,
            rejected: report,
          })
        : undefined;
    root?.addEventListener("paste", paste, { capture: true });
    const stop = this.listenInput(listener);
    this.keyInputEnabled = true;
    this.terminal.options.disableStdin = false;
    this.terminal.focus();
    const close = () => {
      if (closed) return;
      closed = true;
      this.keyInputEnabled = false;
      stopRightClick?.();
      gate?.close();
      this.terminal.options.disableStdin = true;
      stop();
      root?.removeEventListener("paste", paste, { capture: true });
    };
    this.cleanup.push(close);
    return close;
  }
  /** Read the public rendered grid and available host, never private xterm core.
   * Metadata only: a proposed grid is NOT a PTY-confirmed resize permission. */
  measuredDimensions(): Readonly<Geometry> | undefined {
    this.check();
    const root = this.terminal.element,
      host = root?.parentElement;
    if (
      !root?.isConnected ||
      !host ||
      host.ownerDocument.visibilityState !== "visible"
    )
      return;
    const screen = root.querySelector<HTMLElement>(".xterm-screen");
    if (!screen || this.terminal.cols < 2 || this.terminal.rows < 2) return;
    const rect = screen.getBoundingClientRect();
    const viewport = root.querySelector<HTMLElement>(".xterm-viewport");
    const gutter = viewport
      ? Math.max(0, viewport.offsetWidth - viewport.clientWidth) || 0
      : 0;
    const proposed = measuredGrid(
      host.clientWidth - gutter,
      host.clientHeight,
      rect.width / this.terminal.cols,
      rect.height / this.terminal.rows,
    );
    return proposed ? Object.freeze(proposed) : undefined;
  }
  /** Local input events only; the authenticated backend must still admit each write. */
  /** Display focus only; never changes stdin or installs input listeners. */
  focusOutput(): boolean {
    try {
      this.check();
      this.terminal.focus();
      return true;
    } catch {
      return false;
    }
  }
  listenInput(listener: (text: string) => void): () => void {
    this.check();
    let ended = false;
    const event = this.terminal.onData((text) => {
      if (!ended && !this.disposed && this.scope.isCurrent()) listener(text);
    });
    const stop = () => {
      if (!ended) {
        ended = true;
        event.dispose();
      }
    };
    this.cleanup.push(stop);
    return stop;
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.lifetime.abort();
    let failed = false;
    for (const dispose of this.cleanup.reverse()) {
      try {
        dispose();
      } catch {
        failed = true;
      }
    }
    try {
      this.terminal.dispose();
    } catch {
      failed = true;
    }
    if (failed) throw new Error("terminal.surface_cleanup_failed");
  }
}
