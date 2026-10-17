import { nonceDocument } from "./features/terminal/nonce-document";
export interface GutterControls {
  enabled(): boolean;
  resize(split: string, boundary: number, percent: number): Promise<void>;
  failed(error: Error): void;
}
/** Pointer motion previews only the separator. One release applies one layout resize. */
export class WorkspaceGutters {
  private readonly events = new AbortController();
  private readonly style: HTMLStyleElement;
  private drag?: {
    element: HTMLElement;
    pointer: number;
    split: string;
    boundary: number;
    row: boolean;
    start: number;
    extent: number;
    initial: number;
    percent: number;
  };
  constructor(
    private readonly root: HTMLElement,
    private readonly controls: GutterControls,
    signal: AbortSignal,
  ) {
    this.style = nonceDocument(root.ownerDocument).createElement("style");
    root.ownerDocument.head.append(this.style);
    const options = { signal: this.events.signal };
    root.addEventListener("pointerdown", this.down, options);
    root.addEventListener("pointermove", this.move, options);
    root.addEventListener("pointerup", this.up, options);
    root.addEventListener("pointercancel", this.cancel, options);
    root.addEventListener("lostpointercapture", this.cancel, options);
    root.addEventListener("keydown", this.key, options);
    signal.addEventListener("abort", () => this.dispose(), options);
    if (signal.aborted) this.dispose();
  }
  private gutter(event: Event): HTMLElement | undefined {
    const target = event.target as HTMLElement | null;
    const gutter = target?.closest?.<HTMLElement>(".workspace-gutter");
    return gutter && this.root.contains(gutter) ? gutter : undefined;
  }
  private down = (event: PointerEvent): void => {
    const element = this.gutter(event);
    if (
      !element ||
      this.drag ||
      event.button !== 0 ||
      !event.isPrimary ||
      !this.controls.enabled()
    )
      return;
    const rect = element.parentElement?.getBoundingClientRect();
    if (!rect) return;
    const row = element.dataset.direction === "row",
      extent = row ? rect.width : rect.height;
    if (extent <= 6) return;
    event.preventDefault();
    const initial = Number(element.getAttribute("aria-valuenow"));
    this.drag = {
      element,
      pointer: event.pointerId,
      split: element.dataset.split ?? "",
      boundary: Number(element.dataset.boundary),
      row,
      start: row ? event.clientX : event.clientY,
      extent: extent - 6,
      initial,
      percent: initial,
    };
    element.focus({ preventScroll: true });
    this.root.setPointerCapture(event.pointerId);
    element.dataset.dragging = "true";
  };
  private move = (event: PointerEvent): void => {
    const drag = this.drag;
    if (!drag || event.pointerId !== drag.pointer) return;
    if (!this.controls.enabled()) {
      this.cancel();
      return;
    }
    event.preventDefault();
    const delta = (drag.row ? event.clientX : event.clientY) - drag.start;
    drag.percent = Math.max(
      20,
      Math.min(80, Math.round(drag.initial + (delta / drag.extent) * 100)),
    );
    const offset = ((drag.percent - drag.initial) * drag.extent) / 100;
    drag.element.setAttribute("aria-valuenow", String(drag.percent));
    this.style.textContent = `#${this.root.id} .workspace-gutter[data-split="${drag.split}"][data-boundary="${drag.boundary}"]{transform:translate${drag.row ? "X" : "Y"}(${offset}px)}`;
  };
  private up = (event: PointerEvent): void => {
    const drag = this.drag;
    if (!drag || event.pointerId !== drag.pointer) return;
    this.move(event);
    const enabled = this.controls.enabled();
    this.cancel();
    if (enabled && drag.percent !== drag.initial)
      this.apply(drag.split, drag.boundary, drag.percent);
  };
  cancel = (): void => {
    const drag = this.drag;
    this.drag = undefined;
    this.style.textContent = "";
    if (!drag) return;
    drag.element.removeAttribute("data-dragging");
    drag.element.setAttribute("aria-valuenow", String(drag.initial));
    if (this.root.hasPointerCapture(drag.pointer))
      this.root.releasePointerCapture(drag.pointer);
  };
  private key = (event: KeyboardEvent): void => {
    const element = this.gutter(event);
    if (!element) return;
    if (event.key === "Escape") {
      this.cancel();
      event.preventDefault();
      return;
    }
    if (
      this.drag ||
      !this.controls.enabled() ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey
    )
      return;
    const row = element.dataset.direction === "row";
    const step =
      event.key === (row ? "ArrowLeft" : "ArrowUp")
        ? -5
        : event.key === (row ? "ArrowRight" : "ArrowDown")
          ? 5
          : 0;
    if (!step && event.key !== "Home" && event.key !== "End") return;
    event.preventDefault();
    event.stopPropagation();
    const current = Number(element.getAttribute("aria-valuenow"));
    const percent =
      event.key === "Home"
        ? 20
        : event.key === "End"
          ? 80
          : Math.max(20, Math.min(80, current + step));
    if (percent !== current)
      this.apply(
        element.dataset.split ?? "",
        Number(element.dataset.boundary),
        percent,
      );
  };
  private apply(split: string, boundary: number, percent: number): void {
    void this.controls
      .resize(split, boundary, percent)
      .catch((error) =>
        this.controls.failed(
          error instanceof Error ? error : Error("workspace.layout_failed"),
        ),
      );
  }
  dispose(): void {
    this.cancel();
    this.events.abort();
    this.style.remove();
  }
}
