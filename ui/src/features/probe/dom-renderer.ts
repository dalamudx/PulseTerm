import { Terminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import type { AttachBoundary } from "../../ipc/attachment";
import type { DomEvidence } from "../../ipc/gen/commands";
import { nonceDocument } from "../terminal/nonce-document";
import { renderFence } from "../terminal/render-fence";
export interface DomRenderer {
  terminal: Terminal;
  settle(signal: AbortSignal): Promise<void>;
  evidence(): DomEvidence;
  dispose(): void;
}
/** Fixed read-only developer surface. No keyboard/paste input is sent to PTY. */
export function createDomRenderer(
  host: HTMLElement,
  boundary: AttachBoundary,
  signal: AbortSignal,
): DomRenderer {
  if (signal.aborted || !host.isConnected)
    throw new Error("terminal.dom_unavailable");
  host.replaceChildren();
  const documentOverride = nonceDocument(host.ownerDocument);
  const terminal = new Terminal({
    documentOverride,
    cols: boundary.columns,
    rows: boundary.rows,
    scrollback: 128,
    allowProposedApi: true,
    disableStdin: true,
    screenReaderMode: true,
    cursorBlink: false,
    fontSize: 14,
    fontFamily: "monospace",
    theme: {
      background: "#0f172a",
      foreground: "#f1f5f9",
      cursor: "#f1f5f9",
      selectionBackground: "#475569",
    },
  });
  let renders = 0,
    fences = 0,
    rowsChecked = 0,
    disposed = false,
    width = 0,
    height = 0;
  const render = terminal.onRender(() => renders++);
  terminal.open(host);
  terminal.textarea?.setAttribute("aria-label", "只读 PTY 验证输出");
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    signal.removeEventListener("abort", dispose);
    render.dispose();
    terminal.dispose();
    host.replaceChildren();
  };
  signal.addEventListener("abort", dispose, { once: true });
  return {
    terminal,
    dispose,
    async settle(renderSignal) {
      await renderFence(renderSignal, () => !disposed && host.isConnected);
      const screen =
        terminal.element?.querySelector<HTMLElement>(".xterm-screen");
      const rect = screen?.getBoundingClientRect();
      if (
        !rect ||
        rect.width <= 0 ||
        rect.height <= 0 ||
        rect.bottom > window.innerHeight ||
        rect.right > window.innerWidth
      )
        throw new Error("terminal.dom_geometry");
      width = rect.width;
      height = rect.height;
      fences++;
    },
    evidence() {
      if (disposed || renders < 1 || fences < 1)
        throw new Error("terminal.dom_not_rendered");
      const rows = terminal.element?.querySelector(".xterm-rows")?.children;
      if (!rows || rows.length !== terminal.rows)
        throw new Error("terminal.dom_rows");
      const b = terminal.buffer.active;
      for (let i = 0; i < terminal.rows; i++) {
        const expected =
          b.getLine(b.viewportY + i)?.translateToString(true) ?? "";
        const text = (rows[i]?.textContent ?? "")
          .replaceAll("\u00a0", " ")
          .trimEnd();
        if (text !== expected) throw new Error("terminal.dom_projection");
        rowsChecked++;
      }
      return {
        renderEvents: renders,
        frameFences: fences,
        rowsChecked,
        widthPx: width,
        heightPx: height,
        readonly: true,
      };
    },
  };
}
