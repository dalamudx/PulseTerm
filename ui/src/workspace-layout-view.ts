import { nonceDocument } from "./features/terminal/nonce-document";
import { type GutterControls, WorkspaceGutters } from "./workspace-gutters";
import { message, uiAttribute } from "./workspace-language";
import {
  decodePaneLayout,
  type PaneLeaf,
  type PaneNode,
  paneLeaves,
} from "./workspace-layout";
export interface PaneHandle {
  element: HTMLElement;
  mount(): Promise<void>;
  setVisible?(visible: boolean): Promise<void>;
  dispose(): void;
}
/** Stable leaf nodes; only layout wrappers are rebuilt. No arbitrary CSS or inline style. */
export class WorkspaceLayoutView {
  private readonly style: HTMLStyleElement;
  private readonly handles = new Map<
    string,
    { session: string; handle: PaneHandle; mounted: boolean }
  >();
  private disposed = false;
  private readonly gutters?: WorkspaceGutters;
  private readonly abort = () => this.dispose();
  constructor(
    private readonly root: HTMLElement,
    private readonly signal: AbortSignal,
    private readonly factory: (leaf: PaneLeaf) => PaneHandle,
    private readonly failed: (error: Error) => void,
    private readonly controls?: GutterControls,
  ) {
    if (!/^[a-zA-Z0-9_-]{1,64}$/.test(root.id))
      throw Error("workspace.layout_container");
    this.style = nonceDocument(root.ownerDocument).createElement("style");
    root.ownerDocument.head.append(this.style);
    if (controls) this.gutters = new WorkspaceGutters(root, controls, signal);
    signal.addEventListener("abort", this.abort, { once: true });
    if (signal.aborted) this.dispose();
  }
  async render(
    encoded: unknown,
    sessions: readonly string[],
    maximized?: string,
  ): Promise<void> {
    if (this.disposed) throw Error("view.retired");
    const tree = decodePaneLayout(encoded, sessions),
      leaves = paneLeaves(tree),
      created = new Map<
        string,
        { session: string; handle: PaneHandle; mounted: boolean }
      >();
    if (maximized && !leaves.some((leaf) => leaf.view.sessionId === maximized))
      throw Error("workspace.pane_missing");
    this.gutters?.cancel();
    try {
      for (const leaf of leaves) {
        const old = this.handles.get(leaf.id);
        if (old && old.session !== leaf.view.sessionId)
          throw Error("workspace.pane_identity_changed");
        if (!old) {
          const handle = this.factory(leaf);
          created.set(leaf.id, {
            session: leaf.view.sessionId,
            handle,
            mounted: false,
          });
        }
      }
    } catch (error) {
      for (const entry of created.values()) entry.handle.dispose();
      throw error;
    }
    const rules: string[] = [];
    const build = (node: PaneNode): HTMLElement => {
      if (node.type === "leaf") {
        const entry = this.handles.get(node.id) ?? created.get(node.id);
        if (!entry) throw Error("workspace.pane_missing");
        entry.handle.element.classList.add("workspace-pane");
        entry.handle.element.dataset.pane = node.id;
        return entry.handle.element;
      }
      const element = this.root.ownerDocument.createElement("div");
      element.className = "workspace-split";
      element.dataset.node = node.id;
      const tracks = node.sizes
        .map((size) => `minmax(0,${size}fr)`)
        .join(this.controls ? " 6px " : " ");
      rules.push(
        `#${this.root.id} .workspace-split[data-node="${node.id}"]{display:grid;min-width:0;min-height:0;gap:${this.controls ? 0 : 6}px;grid-template-${node.dir === "row" ? "columns" : "rows"}:${tracks}}`,
      );
      for (const [index, child] of node.children.entries()) {
        element.append(build(child));
        if (this.controls && index < node.children.length - 1) {
          const gutter = this.root.ownerDocument.createElement("div");
          gutter.className = "workspace-gutter";
          gutter.dataset.split = node.id;
          gutter.dataset.boundary = String(index);
          gutter.dataset.direction = node.dir;
          gutter.setAttribute("role", "separator");
          gutter.setAttribute("tabindex", "0");
          uiAttribute(
            gutter,
            "aria-label",
            message("调整分屏比例 {0}", node.id),
          );
          gutter.setAttribute(
            "aria-orientation",
            node.dir === "row" ? "vertical" : "horizontal",
          );
          gutter.setAttribute("aria-valuemin", "20");
          gutter.setAttribute("aria-valuemax", "80");
          gutter.setAttribute(
            "aria-valuenow",
            String(Math.round((node.sizes[index] ?? 0.5) * 100)),
          );
          element.append(gutter);
        }
      }
      return element;
    };
    let next: HTMLElement;
    try {
      if (maximized) {
        const visible = leaves.find(
          (leaf) => leaf.view.sessionId === maximized,
        );
        if (!visible) throw Error("workspace.pane_missing");
        next = this.root.ownerDocument.createElement("div");
        next.className = "workspace-maximized";
        next.append(build(visible));
        const hidden = this.root.ownerDocument.createElement("div");
        hidden.setAttribute("hidden", "");
        hidden.setAttribute("inert", "");
        for (const leaf of leaves)
          if (leaf !== visible) hidden.append(build(leaf));
        next.append(hidden);
      } else next = build(tree);
      this.style.textContent = rules.join("\n");
      this.root.replaceChildren(next);
    } catch (error) {
      for (const entry of created.values()) entry.handle.dispose();
      throw error;
    }
    const kept = new Set(leaves.map((leaf) => leaf.id));
    for (const [id, entry] of this.handles) {
      if (!kept.has(id)) {
        entry.handle.dispose();
        this.handles.delete(id);
      }
    }
    for (const [id, entry] of created) this.handles.set(id, entry);
    const updating = [...this.handles.entries()];
    const results = await Promise.allSettled(
      updating.map(([, entry]) => {
        const visible = !maximized || entry.session === maximized;
        const changed = entry.handle.setVisible?.(visible) ?? Promise.resolve();
        if (entry.mounted || !visible) return changed;
        // New hidden leaves still defer their first capture until shown.
        entry.mounted = true;
        return Promise.all([changed, entry.handle.mount()]);
      }),
    );
    if (this.disposed) return;
    for (const [index, result] of results.entries()) {
      const started = updating[index];
      if (!started || this.handles.get(started[0]) !== started[1]) continue;
      if (result.status === "rejected") {
        const error =
          result.reason instanceof Error
            ? result.reason
            : Error("terminal.connection_failed");
        try {
          this.failed(error);
        } catch {
          /* UI notification cannot grant permission. */
        }
      }
    }
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.signal.removeEventListener("abort", this.abort);
    for (const entry of this.handles.values()) entry.handle.dispose();
    this.handles.clear();
    this.gutters?.dispose();
    this.style.remove();
  }
}
