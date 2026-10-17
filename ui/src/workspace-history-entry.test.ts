import { expect, it, vi } from "vitest";
import {
  captureWorkspaceCommands,
  type WorkspaceCommand,
} from "./workspace-command-catalog";
import type { showWorkspaceHistory } from "./workspace-history";
import {
  createWorkspaceHistoryEntry,
  type HistoryEntryTarget,
} from "./workspace-history-entry";

function fixture() {
  const life = new AbortController();
  const focused = {
    isConnected: true,
    focus: vi.fn(),
  } as unknown as HTMLElement;
  const fallback = {
    isConnected: true,
    focus: vi.fn(),
  } as unknown as HTMLElement;
  let nonce: string | undefined = "page-a",
    available = true;
  let target: HistoryEntryTarget | undefined = {
    view: {},
    session: "pane-a",
    layout: "layout-1",
  };
  const document = { activeElement: focused } as unknown as Document;
  const show = vi.fn<typeof showWorkspaceHistory>();
  const entry = createWorkspaceHistoryEntry(
    {
      document,
      signal: life.signal,
      fallbackOpener: fallback,
      nonce: () => nonce,
      available: () => available,
      target: () => target,
    },
    show,
  );
  const command: WorkspaceCommand = {
    id: "find",
    title: "查找当前终端",
    keywords: "search 历史",
    available: () => available && !!target,
    target: entry.target,
    run: entry.open,
  };
  const capture = () =>
    captureWorkspaceCommands(
      { signal: life.signal, nonce: () => nonce, available: () => available },
      [command],
    );
  return {
    life,
    focused,
    fallback,
    show,
    entry,
    capture,
    setNonce: (value: string | undefined) => {
      nonce = value;
    },
    setAvailable: (value: boolean) => {
      available = value;
    },
    setTarget: (value: HistoryEntryTarget | undefined) => {
      target = value;
    },
    getTarget: () => target,
  };
}
it("opens the focused pane and retains its actual keyboard opener", () => {
  const f = fixture();
  const c = f.capture();
  expect(c?.list("查找")).toEqual([{ id: "find", title: "查找当前终端" }]);
  expect(c?.execute("find")).toBe(true);
  expect(f.show).toHaveBeenCalledOnce();
  const options = f.show.mock.calls[0]?.[0];
  expect(options?.session).toBe("pane-a");
  expect(options?.opener).toBe(f.focused);
  expect(options?.current()).toBe(true);
  expect(c?.execute("find")).toBe(false);
});
it("rejects an old palette choice after pane, layout or view replacement", () => {
  for (const change of ["pane", "layout", "view"]) {
    const f = fixture(),
      c = f.capture(),
      old = f.getTarget();
    if (!old) throw Error("missing target");
    f.setTarget({
      ...old,
      ...(change === "pane"
        ? { session: "pane-b" }
        : change === "layout"
          ? { layout: "layout-2" }
          : { view: {} }),
    });
    expect(c?.list("")).toEqual([]);
    expect(c?.execute("find")).toBe(false);
    expect(f.show).not.toHaveBeenCalled();
    expect(f.capture()?.execute("find")).toBe(true);
  }
});
it("invalidates ongoing read context after page, pane, layout, busy or lifetime changes", () => {
  for (const change of ["page", "pane", "layout", "view", "busy", "abort"]) {
    const f = fixture();
    f.entry.open();
    const options = f.show.mock.calls[0]?.[0],
      old = f.getTarget();
    if (!options || !old) throw Error("missing capture");
    if (change === "page") f.setNonce("page-b");
    if (change === "busy") f.setAvailable(false);
    if (change === "abort") f.life.abort();
    if (change === "pane") f.setTarget({ ...old, session: "pane-b" });
    if (change === "layout") f.setTarget({ ...old, layout: "layout-2" });
    if (change === "view") f.setTarget({ ...old, view: {} });
    expect(options.current()).toBe(false);
  }
});
it("does not open without a target, current page or availability", () => {
  for (const change of ["page", "target", "busy", "abort"]) {
    const f = fixture();
    if (change === "page") f.setNonce(undefined);
    if (change === "target") f.setTarget(undefined);
    if (change === "busy") f.setAvailable(false);
    if (change === "abort") f.life.abort();
    f.entry.open();
    expect(f.show).not.toHaveBeenCalled();
  }
});
it("uses the visible toolbar fallback when the original focus is disconnected", () => {
  const f = fixture();
  Object.defineProperty(f.focused, "isConnected", { value: false });
  f.entry.open();
  expect(f.show.mock.calls[0]?.[0].opener).toBe(f.fallback);
});

it("different layout objects cannot reuse an old command choice", () => {
  const f = fixture(),
    old = f.getTarget();
  if (!old) throw Error("missing target");
  f.setTarget({ ...old, layout: {} });
  const c = f.capture();
  f.setTarget({ ...old, layout: {} });
  expect(c?.execute("find")).toBe(false);
  expect(f.show).not.toHaveBeenCalled();
});
