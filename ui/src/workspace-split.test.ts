import { beforeEach, expect, it, vi } from "vitest";
import { type PaneLeaf, type PaneNode, paneLeaves } from "./workspace-layout";
import type { PaneHandle } from "./workspace-layout-view";
import { WorkspaceSplit } from "./workspace-split";

const mock = vi.hoisted(() => ({
  panes: new Map<
    string,
    {
      phase: (state: string, error?: Error) => void;
      focus: ReturnType<typeof vi.fn>;
      disposed: boolean;
    }
  >(),
  factories: 0,
  mounts: 0,
  gate: undefined as Promise<void> | undefined,
}));
vi.mock("./features/terminal/workspace-pane", () => ({
  WorkspacePane: class {
    readonly entry;
    constructor(
      readonly options: {
        session: string;
        phase: (state: string, error?: Error) => void;
      },
    ) {
      this.entry = {
        phase: options.phase,
        focus: vi.fn(() => !this.entry.disposed),
        disposed: false,
      };
      mock.panes.set(options.session, this.entry);
    }
    async mount() {
      mock.mounts++;
      this.options.phase("ready");
    }
    focusOutput() {
      return this.entry.focus();
    }
    dispose() {
      this.entry.disposed = true;
    }
  },
}));
vi.mock("./workspace-layout-view", () => ({
  WorkspaceLayoutView: class {
    readonly handles = new Map<string, PaneHandle>();
    constructor(
      _root: HTMLElement,
      _signal: AbortSignal,
      readonly factory: (leaf: PaneLeaf) => PaneHandle,
    ) {}
    async render(tree: PaneNode) {
      for (const leaf of paneLeaves(tree)) {
        if (!this.handles.has(leaf.id)) {
          mock.factories++;
          const handle = this.factory(leaf);
          this.handles.set(leaf.id, handle);
          await handle.mount();
        }
      }
      await mock.gate;
    }
    dispose() {
      for (const handle of this.handles.values()) handle.dispose();
      this.handles.clear();
    }
  },
}));
beforeEach(() => {
  mock.panes.clear();
  mock.factories = 0;
  mock.mounts = 0;
  mock.gate = undefined;
});
function fixture(
  direction: "row" | "column" = "row",
  catalog: readonly string[] = ["1", "2"],
) {
  const signal = new AbortController(),
    changed = vi.fn();
  const section = () => ({
    ownerDocument: element.ownerDocument,
    id: "",
    dataset: {},
    setAttribute: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
  });
  const element = {
    ownerDocument: { createElement: section },
  } as unknown as HTMLElement;
  const split = new WorkspaceSplit({
    nonce: "backend-page-nonce",
    first: "1",
    second: "2",
    direction,
    catalog,
    element,
    signal: signal.signal,
    paste: { confirm: async () => false },
    changed,
  });
  return { split, signal, changed };
}
it("bounded ratio reuses leaves and restores original focus without a remount", async () => {
  const { split } = fixture();
  expect(split.canAdjustRatio).toBe(false);
  await split.mount();
  expect(split.focusDirection("right")).toBe(true);
  await split.setRatio(60);
  expect(split.ratio).toBe(60);
  expect(split.focusedSession).toBe("2");
  expect(mock.factories).toBe(2);
  expect(mock.mounts).toBe(2);
  expect(mock.panes.get("2")?.focus).toHaveBeenCalledTimes(2);
  for (const ratio of [0, 19, 81, Infinity, NaN, 50.5])
    await expect(split.setRatio(ratio)).rejects.toThrow(
      "workspace.layout_ratio",
    );
  split.dispose();
});
it("direction geometry and refusal do not change focus", async () => {
  const { split } = fixture("column");
  await split.mount();
  expect(split.focusDirection("right")).toBe(false);
  expect(split.focusDirection("down")).toBe(true);
  mock.panes.get("1")?.focus.mockReturnValue(false);
  expect(split.focusDirection("up")).toBe(false);
  expect(split.focusedSession).toBe("2");
  split.dispose();
  expect(split.focusDirection("up")).toBe(false);
});
it("ended or resizing panes never become layout-resize authority", async () => {
  const { split } = fixture();
  await split.mount();
  mock.panes.get("1")?.phase("ended");
  expect(split.canAdjustRatio).toBe(false);
  await expect(split.setRatio(40)).rejects.toThrow(
    "workspace.layout_not_ready",
  );
  mock.panes.get("1")?.phase("resizing");
  expect(split.canAdjustRatio).toBe(false);
  split.dispose();
});
it("ratio operation single-flights and retired completion cannot publish", async () => {
  const { split, changed, signal } = fixture();
  await split.mount();
  let finish!: () => void;
  mock.gate = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const pending = split.setRatio(60);
  expect(split.canAdjustRatio).toBe(false);
  await expect(split.setRatio(40)).rejects.toThrow(
    "workspace.layout_not_ready",
  );
  signal.abort();
  const count = changed.mock.calls.length;
  finish();
  await pending;
  expect(changed).toHaveBeenCalledTimes(count);
  expect(split.ratio).toBe(50);
  expect(split.focusDirection("right")).toBe(false);
});
it("selecting an existing pane preserves both leases and never remounts", async () => {
  const { split } = fixture();
  expect(split.hasSession("1")).toBe(false);
  await split.mount();
  expect(split.hasSession("1")).toBe(true);
  expect(split.focusSession("2")).toBe(true);
  expect(split.focusedSession).toBe("2");
  expect(split.focusSession("3")).toBe(false);
  expect(split.focusedSession).toBe("2");
  mock.panes.get("1")?.phase("ended");
  expect(split.focusSession("1")).toBe(true);
  expect(split.canAdjustRatio).toBe(false);
  expect(mock.factories).toBe(2);
  expect(mock.mounts).toBe(2);
  split.dispose();
  expect(split.focusSession("2")).toBe(false);
});
it("resizing or in-flight ratio adjustment never accepts pane focus", async () => {
  const { split } = fixture();
  await split.mount();
  mock.panes.get("2")?.phase("resizing");
  expect(split.focusSession("2")).toBe(false);
  mock.panes.get("2")?.phase("ready");
  let finish!: () => void;
  mock.gate = new Promise<void>((resolve) => {
    finish = resolve;
  });
  const work = split.setRatio(60);
  expect(split.focusSession("2")).toBe(false);
  finish();
  await work;
  expect(split.focusSession("2")).toBe(true);
  expect(mock.factories).toBe(2);
  split.dispose();
});

it("nested addition and local ratio preserve other panes; maximum display never remounts", async () => {
  const { split } = fixture("row", ["1", "2", "3"]);
  await split.mount();
  await split.addSession("3", "column", ["1", "2", "3"]);
  expect(
    paneLeaves(split.preference!.tree).map((leaf) => leaf.view.sessionId),
  ).toEqual(["1", "3", "2"]);
  const capturedLayout = split.layoutIdentity;
  expect(split.focusSession("1")).toBe(true);
  expect(split.layoutIdentity).toBe(capturedLayout);
  await split.setRatio(65);
  expect(split.layoutIdentity).not.toBe(capturedLayout);
  expect(split.ratio).toBe(65);
  expect(
    split.preference!.tree.type === "split" && split.preference!.tree.sizes,
  ).toEqual([0.5, 0.5]);
  await expect(split.addSession("2", "row", ["1", "2", "3"])).rejects.toThrow(
    "already_displayed",
  );
  await split.toggleMaximized();
  expect(split.preference?.maximized).toBe("1");
  expect(split.focusSession("2")).toBe(false);
  expect(split.focusDirection("down")).toBe(false);
  expect(split.canAdjustRatio).toBe(false);
  await split.toggleMaximized();
  expect(split.focusSession("3")).toBe(true);
  expect(mock.factories).toBe(3);
  expect(mock.mounts).toBe(3);
  split.dispose();
});
it("changing direction of an existing adjacent pair never duplicates a session", async () => {
  const { split } = fixture();
  await split.mount();
  await split.addSession("2", "column", ["1", "2"]);
  expect(
    split.preference?.tree.type === "split" && split.preference.tree.dir,
  ).toBe("column");
  expect(split.focusDirection("down")).toBe(true);
  expect(mock.mounts).toBe(2);
  split.dispose();
});

it("maximum remains restorable while hidden leaves await their first visible attachment", async () => {
  const { split } = fixture();
  await split.mount();
  await split.toggleMaximized();
  mock.panes.get("2")!.phase("loading");
  expect(split.state).toBe("ready");
  expect(split.canChangeLayout).toBe(true);
  await split.toggleMaximized();
  expect(split.state).toBe("loading");
  expect(split.canChangeLayout).toBe(false);
  mock.panes.get("2")!.phase("ready");
  expect(split.state).toBe("ready");
  split.dispose();
});

it("keeps a visible failed pane's reason through sibling updates and focus, then clears it on recovery", async () => {
  const { split, changed } = fixture();
  await split.mount();
  const failure = Error("input.uncertain");
  mock.panes.get("2")!.phase("failed", failure);
  expect(changed).toHaveBeenLastCalledWith("failed", "1", failure);
  mock.panes.get("1")!.phase("ready");
  expect(split.focusSession("1")).toBe(true);
  expect(changed).toHaveBeenLastCalledWith("failed", "1", failure);
  mock.panes.get("2")!.phase("ready");
  expect(changed).toHaveBeenLastCalledWith("ready", "1", undefined);
  split.dispose();
});

it("does not show a hidden pane's failure while maximized, and restores its reason when visible", async () => {
  const { split, changed } = fixture();
  await split.mount();
  await split.toggleMaximized();
  const failure = Error("terminal.font_timeout");
  mock.panes.get("2")!.phase("failed", failure);
  expect(changed).toHaveBeenLastCalledWith("ready", "1", undefined);
  await split.toggleMaximized();
  expect(changed).toHaveBeenLastCalledWith("failed", "1", failure);
  split.dispose();
});
