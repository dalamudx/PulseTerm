import { expect, it } from "vitest";
import { type PaneLeaf, splitPane } from "./workspace-layout";
import {
  loadPanePreference,
  savePanePreference,
} from "./workspace-layout-preferences";

const leaf = (sessionId: string): PaneLeaf => ({
  type: "leaf",
  id: `pane_${sessionId}`,
  view: { kind: "terminal", sessionId },
});
function storage() {
  let text = "";
  return {
    getItem: () => text,
    setItem: (_key: string, value: string) => {
      text = value;
    },
    removeItem: () => {
      text = "";
    },
  };
}
it("restores nested ratios and maximum view, prunes closed sessions rather than launching them", () => {
  const store = storage(),
    tree = splitPane(
      splitPane(leaf("1"), "pane_1", leaf("2"), "root", "row"),
      "pane_2",
      leaf("3"),
      "inner",
      "column",
    );
  savePanePreference({ tree, focused: "3", maximized: "3" }, store);
  expect(loadPanePreference(["1", "2", "3"], store)).toEqual({
    tree,
    focused: "3",
    maximized: "3",
  });
  const restored = loadPanePreference(["1", "2"], store);
  expect(restored?.focused).toBe("1");
  expect(restored?.maximized).toBeUndefined();
  expect(restored?.tree).toEqual(
    splitPane(leaf("1"), "pane_1", leaf("2"), "root", "row"),
  );
  expect(loadPanePreference([], store)).toBeUndefined();
});
it("corruption, duplicate sessions and oversized preferences cannot restore views", () => {
  const read = (text: string) =>
    loadPanePreference(["1", "2"], { getItem: () => text });
  expect(read("{")).toBeUndefined();
  expect(read(" ".repeat(32769))).toBeUndefined();
  const tree = {
    type: "split",
    id: "s",
    dir: "row",
    children: [leaf("1"), leaf("1")],
    sizes: [0.5, 0.5],
  };
  expect(
    read(JSON.stringify({ version: 1, tree, focused: "1", maximized: null })),
  ).toBeUndefined();
  expect(
    read(
      JSON.stringify({
        version: 2,
        tree: leaf("1"),
        focused: "1",
        maximized: null,
      }),
    ),
  ).toBeUndefined();
});
it("unavailable window storage is optional and single view clears the saved layout", () => {
  const store = storage();
  savePanePreference({ tree: leaf("1"), focused: "1" }, store);
  savePanePreference(undefined, store);
  expect(loadPanePreference(["1"], store)).toBeUndefined();
  expect(
    loadPanePreference(["1"], {
      getItem: () => {
        throw Error("denied");
      },
    }),
  ).toBeUndefined();
});
