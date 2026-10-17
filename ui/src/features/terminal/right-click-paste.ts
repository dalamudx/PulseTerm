const pendingReads = new WeakSet<Document>();
export interface RightClickPasteOptions {
  signal: AbortSignal;
  current(): boolean;
  enabled(): boolean;
  mouseMode(): string | undefined;
  focus(): void;
  readText(signal: AbortSignal): Promise<string>;
  submit(text: string): Promise<unknown>;
  rejected(code: string): void;
}
/** User gesture only; a preference never grants native input authority. */
export function bindRightClickPaste(
  root: HTMLElement,
  options: RightClickPasteOptions,
): () => void {
  const doc = root.ownerDocument;
  let closed = false;
  let request: AbortController | undefined;
  let pending = false;
  const retireRead = () => request?.abort();
  const visible = () =>
    root.isConnected &&
    !doc.hidden &&
    doc.visibilityState === "visible" &&
    doc.hasFocus();
  const context = (event: Event) => {
    const mouse = event as MouseEvent;
    if (
      closed ||
      options.signal.aborted ||
      !event.isTrusted ||
      mouse.button !== 2 ||
      mouse.ctrlKey ||
      mouse.shiftKey ||
      mouse.altKey ||
      mouse.metaKey ||
      !options.enabled() ||
      !options.current() ||
      !visible() ||
      options.mouseMode() !== "none"
    )
      return;
    event.preventDefault();
    event.stopImmediatePropagation();
    if (pending || pendingReads.has(doc)) {
      options.rejected("paste.busy");
      return;
    }
    options.focus();
    if (!root.contains(doc.activeElement)) return;
    pending = true;
    pendingReads.add(doc);
    const attempt = new AbortController();
    request = attempt;
    let timer: ReturnType<typeof setTimeout> | undefined = setTimeout(() => {
      attempt.abort();
      if (!closed && options.current())
        options.rejected("paste.clipboard_unavailable");
    }, 5000);
    void (async () => {
      try {
        const text = await options.readText(attempt.signal);
        clearTimeout(timer);
        timer = undefined;
        if (
          attempt.signal.aborted ||
          closed ||
          options.signal.aborted ||
          !options.current() ||
          !options.enabled() ||
          !visible() ||
          !root.contains(doc.activeElement) ||
          options.mouseMode() !== "none"
        )
          return;
        // The existing PasteGate now owns approval and view cancellation.
        request = undefined;
        if (text) await options.submit(text);
      } catch (error) {
        if (
          !attempt.signal.aborted &&
          !closed &&
          !options.signal.aborted &&
          options.current()
        )
          options.rejected(
            error instanceof Error &&
              error.message === "paste.clipboard_api_missing"
              ? "paste.clipboard_api_missing"
              : "paste.clipboard_unavailable",
          );
      } finally {
        clearTimeout(timer);
        if (request === attempt) request = undefined;
        // Cancellation cannot release the slot before the browser read settles.
        pendingReads.delete(doc);
        pending = false;
      }
    })();
  };
  const close = () => {
    if (closed) return;
    closed = true;
    retireRead();
    root.removeEventListener("contextmenu", context, true);
    root.removeEventListener("focusout", retireRead, true);
    doc.removeEventListener("visibilitychange", retireRead);
    doc.defaultView?.removeEventListener("blur", retireRead);
    options.signal.removeEventListener("abort", close);
  };
  root.addEventListener("contextmenu", context, true);
  root.addEventListener("focusout", retireRead, true);
  doc.addEventListener("visibilitychange", retireRead);
  doc.defaultView?.addEventListener("blur", retireRead);
  options.signal.addEventListener("abort", close, { once: true });
  if (options.signal.aborted) close();
  return close;
}
