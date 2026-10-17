import { decodeWorkspaceSessions } from "./workspace-sessions";
export type PaneLeaf = Readonly<{
  type: "leaf";
  id: string;
  view: Readonly<{ kind: "terminal"; sessionId: string }>;
}>;
export type PaneSplit = Readonly<{
  type: "split";
  id: string;
  dir: "row" | "column";
  children: readonly PaneNode[];
  sizes: readonly number[];
}>;
export type PaneNode = PaneLeaf | PaneSplit;
function invalid(): never {
  throw Error("workspace.layout_invalid");
}
function record(
  value: unknown,
  keys: readonly string[],
): value is Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return false;
  return (
    Object.keys(value).length === keys.length &&
    keys.every((k) => Object.hasOwn(value, k))
  );
}
function ratios(values: readonly number[]): readonly number[] {
  if (Array.from(values).some((v) => !Number.isFinite(v) || v <= 0)) invalid();
  const sum = values.reduce((a, b) => a + b, 0);
  if (!Number.isFinite(sum) || sum <= 0) invalid();
  const out = values.map((v) => v / sum);
  if (out.some((v) => v < 0.05)) invalid();
  return Object.freeze(out);
}
/** Bounded display model. Catalog membership is NOT backend ownership. */
export function decodePaneLayout(
  value: unknown,
  sessions: readonly string[],
): PaneNode {
  const allowed = new Set(decodeWorkspaceSessions(sessions)),
    ids = new Set<string>(),
    used = new Set<string>(),
    seen = new WeakSet<object>();
  let count = 0;
  const visit = (node: unknown, depth: number): PaneNode => {
    if (
      depth > 16 ||
      ++count > 127 ||
      !node ||
      typeof node !== "object" ||
      seen.has(node)
    )
      invalid();
    seen.add(node);
    const r = node as Record<string, unknown>;
    if (
      typeof r.id !== "string" ||
      !/^[a-zA-Z0-9_-]{1,64}$/.test(r.id) ||
      ids.has(r.id)
    )
      invalid();
    ids.add(r.id);
    if (r.type === "leaf") {
      if (
        !record(node, ["type", "id", "view"]) ||
        !record(r.view, ["kind", "sessionId"]) ||
        r.view.kind !== "terminal" ||
        typeof r.view.sessionId !== "string" ||
        !allowed.has(r.view.sessionId) ||
        used.has(r.view.sessionId) ||
        used.size >= 64
      )
        invalid();
      used.add(r.view.sessionId);
      return Object.freeze({
        type: "leaf",
        id: r.id,
        view: Object.freeze({ kind: "terminal", sessionId: r.view.sessionId }),
      });
    }
    if (
      r.type !== "split" ||
      !record(node, ["type", "id", "dir", "children", "sizes"]) ||
      (r.dir !== "row" && r.dir !== "column") ||
      !Array.isArray(r.children) ||
      r.children.length < 2 ||
      r.children.length > 8 ||
      !Array.isArray(r.sizes) ||
      r.sizes.length !== r.children.length ||
      r.sizes.some((v) => typeof v !== "number")
    )
      invalid();
    return Object.freeze({
      type: "split",
      id: r.id,
      dir: r.dir,
      children: Object.freeze(
        Array.from(r.children).map((child) => visit(child, depth + 1)),
      ),
      sizes: ratios(r.sizes),
    });
  };
  return visit(value, 0);
}
export function paneLeaves(root: PaneNode): readonly PaneLeaf[] {
  const pending = [{ node: root, depth: 0 }],
    seen = new Set<PaneNode>(),
    out: PaneLeaf[] = [];
  while (pending.length) {
    const entry = pending.pop();
    if (!entry) break;
    const { node, depth } = entry;
    if (!node || depth > 16 || seen.has(node) || seen.size >= 127) invalid();
    seen.add(node);
    if (node.type === "leaf") out.push(node);
    else if (
      node.type === "split" &&
      Array.isArray(node.children) &&
      node.children.length <= 8
    ) {
      for (const child of [...node.children].reverse())
        pending.push({ node: child, depth: depth + 1 });
    } else invalid();
  }
  return Object.freeze(out);
}
function checked(root: PaneNode): PaneNode {
  return decodePaneLayout(
    root,
    paneLeaves(root).map((n) => n.view.sessionId),
  );
}
function map(
  root: PaneNode,
  target: string,
  replace: (node: PaneNode) => PaneNode,
): PaneNode {
  if (root.id === target) return replace(root);
  if (root.type === "leaf") return root;
  const children = root.children.map((child) => map(child, target, replace));
  return children.every((child, i) => child === root.children[i])
    ? root
    : Object.freeze({ ...root, children: Object.freeze(children) });
}
export function splitPane(
  root: PaneNode,
  target: string,
  newPane: PaneLeaf,
  splitId: string,
  dir: "row" | "column",
): PaneNode {
  root = checked(root);
  if (!paneLeaves(root).some((n) => n.id === target))
    throw Error("workspace.pane_missing");
  const next = map(root, target, (node) => {
    if (node.type !== "leaf") invalid();
    return {
      type: "split",
      id: splitId,
      dir,
      children: [node, newPane],
      sizes: [0.5, 0.5],
    };
  });
  return decodePaneLayout(next, [
    ...paneLeaves(root).map((n) => n.view.sessionId),
    newPane.view.sessionId,
  ]);
}
/** Removing display layout is not a close-session operation. */
export function removePane(
  root: PaneNode,
  target: string,
): PaneNode | undefined {
  root = checked(root);
  if (!paneLeaves(root).some((n) => n.id === target))
    throw Error("workspace.pane_missing");
  const prune = (node: PaneNode): PaneNode | undefined => {
    if (node.type === "leaf") return node.id === target ? undefined : node;
    const entries = node.children.flatMap((child, i) => {
      const kept = prune(child);
      return kept ? [{ child: kept, size: node.sizes[i] ?? 0 }] : [];
    });
    if (entries.length === 1) return entries[0]?.child;
    if (
      entries.length === node.children.length &&
      entries.every((entry, i) => entry.child === node.children[i])
    )
      return node;
    return Object.freeze({
      ...node,
      children: Object.freeze(entries.map((e) => e.child)),
      sizes: ratios(entries.map((e) => e.size)),
    });
  };
  return prune(root);
}
export function setSplitSizes(
  root: PaneNode,
  split: string,
  sizes: readonly number[],
): PaneNode {
  root = checked(root);
  let found = false;
  const result = map(root, split, (node) => {
    if (node.type !== "split" || sizes.length !== node.children.length)
      invalid();
    found = true;
    return Object.freeze({ ...node, sizes: ratios(sizes) });
  });
  if (!found) throw Error("workspace.pane_missing");
  return result;
}
export function equalizePanes(root: PaneNode): PaneNode {
  root = checked(root);
  const equal = (node: PaneNode): PaneNode =>
    node.type === "leaf"
      ? node
      : Object.freeze({
          ...node,
          children: Object.freeze(node.children.map(equal)),
          sizes: ratios(node.children.map(() => 1)),
        });
  return equal(root);
}
export type PaneDirection = "left" | "right" | "up" | "down";
type Rect = Readonly<{
  leaf: PaneLeaf;
  x: number;
  y: number;
  width: number;
  height: number;
}>;
function rectangles(
  root: PaneNode,
  x = 0,
  y = 0,
  width = 1,
  height = 1,
): Rect[] {
  if (root.type === "leaf") return [{ leaf: root, x, y, width, height }];
  let offset = 0;
  return root.children.flatMap((child, i) => {
    const size = root.sizes[i] ?? 0;
    const out =
      root.dir === "row"
        ? rectangles(child, x + offset * width, y, size * width, height)
        : rectangles(child, x, y + offset * height, width, size * height);
    offset += size;
    return out;
  });
}
export function adjacentPane(
  root: PaneNode,
  current: string,
  direction: PaneDirection,
): string | undefined {
  root = checked(root);
  const entries = rectangles(root),
    source = entries.find((r) => r.leaf.id === current);
  if (!source) throw Error("workspace.pane_missing");
  const horizontal = direction === "left" || direction === "right",
    sign = direction === "right" || direction === "down" ? 1 : -1;
  const center = (r: Rect) =>
      horizontal ? r.x + r.width / 2 : r.y + r.height / 2,
    cross = (r: Rect) => (horizontal ? r.y + r.height / 2 : r.x + r.width / 2);
  return entries
    .filter((r) => r !== source && (center(r) - center(source)) * sign > 1e-9)
    .map((r) => ({
      r,
      score:
        (center(r) - center(source)) * sign +
        Math.abs(cross(r) - cross(source)) * 2,
    }))
    .sort(
      (a, b) => a.score - b.score || a.r.leaf.id.localeCompare(b.r.leaf.id),
    )[0]?.r.leaf.id;
}
