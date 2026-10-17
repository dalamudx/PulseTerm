import type { DisplayPreferences } from "./features/terminal/display-preferences";
import type { TerminalPasteOptions } from "./features/terminal/terminal-surface";
import {
  type PanePhase,
  WorkspacePane,
} from "./features/terminal/workspace-pane";
import { message, uiAttribute } from "./workspace-language";
import {
  adjacentPane,
  decodePaneLayout,
  type PaneDirection,
  type PaneLeaf,
  type PaneNode,
  type PaneSplit,
  paneLeaves,
  setSplitSizes,
  splitPane,
} from "./workspace-layout";
import type { LayoutPreference } from "./workspace-layout-preferences";
import { WorkspaceLayoutView } from "./workspace-layout-view";

function findSplit(root: PaneNode, id: string): PaneSplit | undefined {
  if (root.type === "leaf") return;
  if (root.id === id) return root;
  for (const child of root.children) {
    const found = findSplit(child, id);
    if (found) return found;
  }
}
function parentSplit(root: PaneNode, leaf: string): PaneSplit | undefined {
  if (root.type === "leaf") return;
  if (root.children.some((child) => child.id === leaf)) return root;
  for (const child of root.children) {
    const found = parentSplit(child, leaf);
    if (found) return found;
  }
}

export class WorkspaceSplit {
  private readonly life = new AbortController();
  private readonly layout: WorkspaceLayoutView;
  private readonly phases = new Map<string, PanePhase>();
  private readonly abort = () => this.dispose();
  private disposed = false;
  private focused: string;
  private tree: PaneNode | undefined;
  private adjusting = false;
  private restoreFocus: string | undefined;
  private maximized: string | undefined;
  private catalog: readonly string[];
  private readonly panes = new Map<string, WorkspacePane>();
  constructor(
    private readonly options: {
      nonce: string;
      first: string;
      second: string;
      direction: "row" | "column";
      catalog: readonly string[];
      element: HTMLElement;
      signal: AbortSignal;
      paste: TerminalPasteOptions;
      display?: DisplayPreferences;
      restored?: LayoutPreference;
      changed(state: PanePhase, focused: string, error?: Error): void;
    },
  ) {
    this.focused = options.restored?.focused ?? options.first;
    this.maximized = options.restored?.maximized;
    this.catalog = options.catalog;
    this.layout = new WorkspaceLayoutView(
      options.element,
      this.life.signal,
      (leaf) => {
        const element = options.element.ownerDocument.createElement("section");
        element.id = `workspace-pane-${leaf.view.sessionId}`;
        element.setAttribute("role", "group");
        uiAttribute(
          element,
          "aria-label",
          message("会话 {0}", leaf.view.sessionId),
        );
        element.dataset.state = "loading";
        this.phases.set(leaf.view.sessionId, "loading");
        const focus = () => {
          if (
            !this.disposed &&
            !this.adjusting &&
            (!this.maximized || leaf.view.sessionId === this.maximized)
          ) {
            this.focused = leaf.view.sessionId;
            this.notify();
          }
        };
        element.addEventListener("focusin", focus);
        element.addEventListener("pointerdown", focus);
        const controller = new WorkspacePane({
          nonce: options.nonce,
          session: leaf.view.sessionId,
          element,
          signal: this.life.signal,
          input: true,
          paste: options.paste,
          display: options.display,
          phase: (phase, error) => {
            if (this.disposed || phase === "disposed") return;
            element.dataset.state = phase;
            this.phases.set(leaf.view.sessionId, phase);
            this.notify(error);
            this.restoreDisplayFocus();
          },
        });
        this.panes.set(leaf.id, controller);
        return {
          element,
          mount: () => controller.mount(),
          setVisible: (visible) => controller.setVisible(visible),
          dispose: () => {
            element.removeEventListener("focusin", focus);
            element.removeEventListener("pointerdown", focus);
            controller.dispose();
            if (this.panes.get(leaf.id) === controller)
              this.panes.delete(leaf.id);
            this.phases.delete(leaf.view.sessionId);
          },
        };
      },
      (error) => this.notify(error),
      {
        enabled: () => this.canAdjustRatio,
        resize: (split, boundary, percent) =>
          this.resizeSplit(split, boundary, percent),
        failed: (error) => this.notify(error),
      },
    );
    options.signal.addEventListener("abort", this.abort, { once: true });
    if (options.signal.aborted) this.dispose();
  }
  private get visiblePhases(): readonly PanePhase[] {
    return this.maximized
      ? [this.phases.get(this.maximized) ?? "loading"]
      : [...this.phases.values()];
  }
  get state(): PanePhase {
    if (this.disposed) return "disposed";
    if (this.adjusting) return "resizing";
    const states = this.visiblePhases;
    if (states.includes("failed")) return "failed";
    if (states.includes("loading")) return "loading";
    if (states.includes("resizing")) return "resizing";
    if (states.includes("paused")) return "paused";
    if (states.length && states.every((s) => s === "ended")) return "ended";
    return "ready";
  }
  get focusedSession(): string {
    return this.focused;
  }
  get preference(): LayoutPreference | undefined {
    return this.tree && !this.disposed
      ? { tree: this.tree, focused: this.focused, maximized: this.maximized }
      : undefined;
  }
  get isMaximized(): boolean {
    return !!this.maximized;
  }
  get canChangeLayout(): boolean {
    return (
      !this.disposed &&
      !this.adjusting &&
      !!this.tree &&
      this.phases.size >= 2 &&
      this.visiblePhases.every((state) => state === "ready")
    );
  }
  get canAdjustRatio(): boolean {
    return this.canChangeLayout && !this.maximized;
  }
  get layoutIdentity(): object | undefined {
    return this.tree;
  }
  get canExtend(): boolean {
    return this.canAdjustRatio && this.phases.size < 8;
  }
  get ratio(): number {
    const split = this.tree && parentSplit(this.tree, `pane_${this.focused}`);
    return Math.round((split?.sizes[0] ?? 0.5) * 100);
  }
  async setRatio(percent: number): Promise<void> {
    const split = this.tree && parentSplit(this.tree, `pane_${this.focused}`);
    if (!split) throw Error("workspace.layout_not_ready");
    await this.resizeSplit(split.id, 0, percent);
  }
  private async resizeSplit(
    id: string,
    boundary: number,
    percent: number,
  ): Promise<void> {
    if (!this.canAdjustRatio || !this.tree)
      throw Error("workspace.layout_not_ready");
    if (!Number.isInteger(percent) || percent < 20 || percent > 80)
      throw Error("workspace.layout_ratio");
    const split = findSplit(this.tree, id);
    if (!split || split.children.length !== 2 || boundary !== 0)
      throw Error("workspace.layout_ratio");
    await this.applyLayout(
      setSplitSizes(this.tree, id, [percent, 100 - percent]),
    );
  }
  async addSession(
    session: string,
    direction: "row" | "column",
    catalog: readonly string[],
  ): Promise<void> {
    if (!this.canAdjustRatio || !this.tree)
      throw Error("workspace.layout_not_ready");
    if (this.hasSession(session)) {
      const target = parentSplit(this.tree, `pane_${this.focused}`);
      if (
        !target ||
        !target.children.some(
          (child) => child.type === "leaf" && child.view.sessionId === session,
        )
      )
        throw Error("workspace.pane_already_displayed");
      const orient = (node: PaneNode): PaneNode =>
        node.type === "leaf"
          ? node
          : node.id === target.id
            ? { ...node, dir: direction }
            : { ...node, children: node.children.map(orient) };
      await this.applyLayout(decodePaneLayout(orient(this.tree), catalog));
      return;
    }
    if (!this.canExtend) throw Error("workspace.layout_limit");
    const next = decodePaneLayout(
      splitPane(
        this.tree,
        `pane_${this.focused}`,
        {
          type: "leaf",
          id: `pane_${session}`,
          view: { kind: "terminal", sessionId: session },
        },
        `split_${session}`,
        direction,
      ),
      catalog,
    );
    this.catalog = catalog;
    await this.applyLayout(next);
  }
  async toggleMaximized(): Promise<void> {
    if (!this.canChangeLayout || !this.tree)
      throw Error("workspace.layout_not_ready");
    await this.applyLayout(
      this.tree,
      this.maximized ? undefined : this.focused,
    );
  }
  private async applyLayout(next: PaneNode, maximized?: string): Promise<void> {
    this.adjusting = true;
    this.restoreFocus = this.focused;
    this.notify();
    try {
      await this.layout.render(next, this.catalog, maximized);
      if (!this.disposed) {
        this.tree = next;
        this.maximized = maximized;
      }
    } finally {
      this.adjusting = false;
      this.restoreDisplayFocus();
      this.notify();
    }
  }
  hasSession(session: string): boolean {
    return !this.disposed && !!this.tree && this.panes.has(`pane_${session}`);
  }
  focusSession(session: string): boolean {
    if (
      !this.hasSession(session) ||
      this.adjusting ||
      (this.maximized && session !== this.maximized)
    )
      return false;
    const phase = this.phases.get(session);
    if (phase !== "ready" && phase !== "ended") return false;
    if (!this.panes.get(`pane_${session}`)?.focusOutput()) return false;
    this.focused = session;
    this.notify();
    return true;
  }
  focusDirection(direction: PaneDirection): boolean {
    if (this.disposed || !this.tree || this.adjusting || this.maximized)
      return false;
    const target = adjacentPane(this.tree, `pane_${this.focused}`, direction);
    if (!target) return false;
    if (!this.panes.get(target)?.focusOutput()) return false;
    const leafSession = target.slice("pane_".length);
    this.focused = leafSession;
    this.notify();
    return true;
  }

  private restoreDisplayFocus(): void {
    if (!this.canChangeLayout || !this.restoreFocus) return;
    const target = this.restoreFocus;
    this.restoreFocus = undefined;
    this.panes.get(`pane_${target}`)?.focusOutput();
  }
  private notify(error?: Error) {
    if (this.disposed) return;
    try {
      this.options.changed(this.state, this.focused, error);
    } catch {
      /* Feedback does not authorize native operations. */
    }
  }
  async mount(): Promise<void> {
    if (this.disposed) throw Error("view.retired");
    const first: PaneLeaf = {
        type: "leaf",
        id: `pane_${this.options.first}`,
        view: { kind: "terminal", sessionId: this.options.first },
      },
      second: PaneLeaf = {
        type: "leaf",
        id: `pane_${this.options.second}`,
        view: { kind: "terminal", sessionId: this.options.second },
      };
    this.tree =
      this.options.restored?.tree ??
      splitPane(first, first.id, second, "split_root", this.options.direction);
    this.tree = decodePaneLayout(this.tree, this.catalog);
    if (paneLeaves(this.tree).length > 8) throw Error("workspace.layout_limit");
    await this.layout.render(this.tree, this.catalog, this.maximized);
    this.notify();
  }
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.options.signal.removeEventListener("abort", this.abort);
    this.life.abort();
    this.layout.dispose();
    this.tree = undefined;
    this.panes.clear();
  }
}
