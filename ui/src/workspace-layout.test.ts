import { expect, it } from "vitest";
import {
  adjacentPane,
  decodePaneLayout,
  equalizePanes,
  type PaneLeaf,
  type PaneNode,
  paneLeaves,
  removePane,
  setSplitSizes,
  splitPane,
} from "./workspace-layout";

const leaf = (id: string, sessionId: string): PaneLeaf => ({
  type: "leaf",
  id,
  view: { kind: "terminal", sessionId },
});
it("immutable catalog-only leaves prevent duplicate session leases and node ids", () => {
  const raw = leaf("a", "1"),
    root = decodePaneLayout(raw, ["1"]);
  expect(root).not.toBe(raw);
  expect(Object.isFrozen(root)).toBe(true);
  expect(Object.isFrozen((root as PaneLeaf).view)).toBe(true);
  for (const input of [
    leaf("a", "2"),
    leaf("<script>", "1"),
    { ...raw, program: "/bin/sh" },
    { ...raw, view: { kind: "terminal", sessionId: "1", nonce: "x" } },
  ])
    expect(() => decodePaneLayout(input, ["1"])).toThrow();
  expect(() => splitPane(root, "a", leaf("b", "1"), "s", "row")).toThrow();
  expect(() => splitPane(root, "a", leaf("a", "2"), "s", "row")).toThrow();
});
it("split/close promote sibling without closing a session or mutating source", () => {
  const a = decodePaneLayout(leaf("a", "1"), ["1"]),
    ab = splitPane(a, "a", leaf("b", "2"), "s", "row");
  expect(paneLeaves(ab).map((n) => n.id)).toEqual(["a", "b"]);
  expect(paneLeaves(a).map((n) => n.id)).toEqual(["a"]);
  expect(removePane(ab, "a")).toEqual(leaf("b", "2"));
  expect(removePane(a, "a")).toBeUndefined();
  expect(() => removePane(ab, "missing")).toThrow("pane_missing");
});
it("nested orientation and sizes give deterministic directional focus", () => {
  let root = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  root = splitPane(root, "b", leaf("c", "3"), "s2", "column");
  expect(adjacentPane(root, "a", "right")).toBe("b");
  expect(adjacentPane(root, "b", "down")).toBe("c");
  expect(adjacentPane(root, "c", "up")).toBe("b");
  expect(adjacentPane(root, "a", "left")).toBeUndefined();
  const sized = setSplitSizes(root, "s", [3, 1]);
  expect(sized.type === "split" && sized.sizes).toEqual([0.75, 0.25]);
  const equal = equalizePanes(sized);
  expect(equal.type === "split" && equal.sizes).toEqual([0.5, 0.5]);
  expect(paneLeaves(equal).map((n) => n.view.sessionId)).toEqual([
    "1",
    "2",
    "3",
  ]);
});
it("invalid/starvation sizes and missing target never select fallback", () => {
  const root = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  for (const sizes of [[0, 1], [NaN, 1], [Infinity, 1], [-1, 2], [1], [1, 100]])
    expect(() => setSplitSizes(root, "s", sizes)).toThrow();
  expect(() => setSplitSizes(root, "a", [1, 1])).toThrow();
  expect(() => setSplitSizes(root, "missing", [1, 1])).toThrow("pane_missing");
  expect(() =>
    splitPane(root, "missing", leaf("c", "3"), "s2", "column"),
  ).toThrow("pane_missing");
});
it("cycle/repeated node and excessive depth reject before recursive operations", () => {
  const cycle: {
    type: string;
    id: string;
    dir: string;
    children: unknown[];
    sizes: number[];
  } = { type: "split", id: "cycle", dir: "row", children: [], sizes: [1, 1] };
  cycle.children = [cycle, leaf("a", "1")];
  expect(() => decodePaneLayout(cycle, ["1"])).toThrow();
  expect(() => paneLeaves(cycle as unknown as PaneNode)).toThrow();
  expect(() => removePane(cycle as unknown as PaneNode, "a")).toThrow();
  let node: PaneNode = leaf("a", "1");
  for (let i = 0; i < 18; i++)
    node = {
      type: "split",
      id: `s${i}`,
      dir: "column",
      children: [node, leaf(`p${i}`, String(i + 2))],
      sizes: [1, 1],
    };
  expect(() =>
    decodePaneLayout(
      node,
      Array.from({ length: 19 }, (_, i) => String(i + 1)),
    ),
  ).toThrow();
});
it("100 generated edit streams preserve immutable uniqueness and reversible closes", () => {
  for (let round = 0; round < 100; round++) {
    let root: PaneNode = leaf("a", "1");
    for (let n = 2; n <= 10; n++) {
      const leaves = paneLeaves(root),
        target = leaves[(n + round) % leaves.length];
      if (!target) throw Error("fixture");
      root = splitPane(
        root,
        target.id,
        leaf(`p${n}`, String(n)),
        `s${n}`,
        (round + n) % 2 ? "row" : "column",
      );
      root = equalizePanes(root);
      expect(paneLeaves(root)).toHaveLength(n);
    }
    for (let n = 10; n >= 2; n--) {
      const next = removePane(root, `p${n}`);
      if (!next) throw Error("fixture empty");
      root = next;
      expect(paneLeaves(root)).toHaveLength(n - 1);
    }
    expect(root).toEqual(leaf("a", "1"));
  }
});
it("closing final child of three-way split cannot be mistaken for no change", () => {
  const root = decodePaneLayout(
    {
      type: "split",
      id: "s",
      dir: "row",
      children: [leaf("a", "1"), leaf("b", "2"), leaf("c", "3")],
      sizes: [1, 1, 1],
    },
    ["1", "2", "3"],
  );
  const next = removePane(root, "c");
  expect(next?.type === "split" && next.children.length).toBe(2);
  expect(next?.type === "split" && next.sizes).toEqual([0.5, 0.5]);
  expect(paneLeaves(root)).toHaveLength(3);
});
it("sparse children and sizes reject instead of skipping decoding", () => {
  const root = {
    type: "split",
    id: "s",
    dir: "row",
    children: [leaf("a", "1"), leaf("b", "2")],
    sizes: [1, 1],
  };
  expect(() =>
    decodePaneLayout({ ...root, children: Array(2) }, ["1", "2"]),
  ).toThrow();
  expect(() =>
    decodePaneLayout({ ...root, sizes: Array(2) }, ["1", "2"]),
  ).toThrow();
  expect(() => setSplitSizes(root as PaneNode, "s", Array(2))).toThrow();
});
