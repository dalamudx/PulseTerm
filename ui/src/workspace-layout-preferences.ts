import {
  decodePaneLayout,
  type PaneNode,
  paneLeaves,
  removePane,
} from "./workspace-layout";

const KEY = ["pulseterm", "workspace", "layout", "v1"].join(".");
export type LayoutPreference = Readonly<{
  tree: PaneNode;
  focused: string;
  maximized?: string;
}>;
/** Window-local display preference. Backend catalog and each binding remain authoritative. */
export function loadPanePreference(
  sessions: readonly string[],
  storage?: Pick<Storage, "getItem">,
): LayoutPreference | undefined {
  try {
    const text = storage?.getItem(KEY);
    if (!text || text.length > 32768) return;
    const value = JSON.parse(text);
    if (
      !value ||
      typeof value !== "object" ||
      Array.isArray(value) ||
      Object.keys(value).length !== 4 ||
      value.version !== 1 ||
      typeof value.focused !== "string" ||
      !(value.maximized === null || typeof value.maximized === "string")
    )
      return;
    const leaves = paneLeaves(value.tree);
    if (
      leaves.length > 8 ||
      leaves.some((leaf) => leaf.id !== `pane_${leaf.view.sessionId}`)
    )
      return;
    let tree: PaneNode | undefined = decodePaneLayout(
      value.tree,
      leaves.map((leaf) => leaf.view.sessionId),
    );
    const pending = [tree];
    while (pending.length) {
      const node = pending.pop();
      if (node?.type === "split") {
        if (node.children.length !== 2) return;
        pending.push(...node.children);
      }
    }
    for (const leaf of leaves) {
      if (tree && !sessions.includes(leaf.view.sessionId))
        tree = removePane(tree, leaf.id);
    }
    if (!tree) return;
    tree = decodePaneLayout(tree, sessions);
    const available = paneLeaves(tree).map((leaf) => leaf.view.sessionId);
    const focused = available.includes(value.focused)
      ? value.focused
      : available[0];
    if (!focused) return;
    return {
      tree,
      focused,
      maximized: available.includes(value.maximized)
        ? value.maximized
        : undefined,
    };
  } catch {
    /* Corrupt preferences never start replacement shells. */
  }
}
export function savePanePreference(
  value: LayoutPreference | undefined,
  storage?: Pick<Storage, "setItem" | "removeItem">,
): void {
  try {
    if (!value) {
      storage?.removeItem(KEY);
      return;
    }
    const leaves = paneLeaves(value.tree);
    const tree = decodePaneLayout(
      value.tree,
      leaves.map((leaf) => leaf.view.sessionId),
    );
    if (
      leaves.length > 8 ||
      !leaves.some((leaf) => leaf.view.sessionId === value.focused)
    )
      return;
    storage?.setItem(
      KEY,
      JSON.stringify({
        version: 1,
        tree,
        focused: value.focused,
        maximized: value.maximized ?? null,
      }),
    );
  } catch {
    /* Optional display persistence grants no authority and does not retry. */
  }
}
