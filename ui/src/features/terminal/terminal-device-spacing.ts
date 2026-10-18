interface SpacingTarget {
  options: { letterSpacing?: number };
}

/** xterm's renderers add letterSpacing in device pixels before converting to CSS. */
export function terminalDeviceSpacing(
  cssPixels: number,
  document: Document,
): number {
  const ratio = document.defaultView?.devicePixelRatio;
  return (
    cssPixels *
    (typeof ratio === "number" && Number.isFinite(ratio) && ratio > 0
      ? ratio
      : 1)
  );
}

/** Track only this document; disposal or retirement prevents late option changes. */
export function bindTerminalDeviceSpacing(
  terminal: SpacingTarget,
  document: Document,
  cssPixels: number,
  signal: AbortSignal,
  current: () => boolean,
): () => void {
  const view = document.defaultView;
  if (!view || cssPixels === 0 || signal.aborted || !current()) return () => {};
  let disposed = false;
  let media: MediaQueryList | undefined;
  const dispose = () => {
    if (disposed) return;
    disposed = true;
    media?.removeEventListener("change", update);
    view.removeEventListener("resize", update);
    signal.removeEventListener("abort", dispose);
  };
  const update = () => {
    if (disposed) return;
    if (!current()) {
      dispose();
      return;
    }
    const spacing = terminalDeviceSpacing(cssPixels, document);
    if (terminal.options.letterSpacing !== spacing)
      terminal.options.letterSpacing = spacing;
    if (typeof view.matchMedia !== "function") return;
    const query = `(resolution: ${terminalDeviceSpacing(1, document)}dppx)`;
    if (media?.media === query) return;
    media?.removeEventListener("change", update);
    media = view.matchMedia(query);
    media.addEventListener("change", update);
  };
  view.addEventListener("resize", update);
  signal.addEventListener("abort", dispose, { once: true });
  update();
  return dispose;
}
