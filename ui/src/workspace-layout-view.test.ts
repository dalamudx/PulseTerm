import { expect, it, vi } from "vitest";
import type { GutterControls } from "./workspace-gutters";
import { applyUiLanguage } from "./workspace-language";
import {
  decodePaneLayout,
  type PaneLeaf,
  setSplitSizes,
  splitPane,
} from "./workspace-layout";
import { type PaneHandle, WorkspaceLayoutView } from "./workspace-layout-view";

class Node {
  id = "";
  className = "";
  textContent = "";
  dataset: Record<string, string> = {};
  children: Node[] = [];
  attributes = new Map<string, string>();
  classList = { add: vi.fn() };
  removed = false;
  constructor(readonly ownerDocument: Doc) {}
  get ariaLabel() {
    return this.attributes.get("aria-label") ?? "";
  }
  set ariaLabel(value: string) {
    this.attributes.set("aria-label", value);
  }
  addEventListener = vi.fn();
  get isConnected() {
    return !this.removed;
  }
  setAttribute(k: string, v: string) {
    this.attributes.set(k, v);
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  remove() {
    this.removed = true;
    this.ownerDocument.head.children = this.ownerDocument.head.children.filter(
      (n) => n !== this,
    );
  }
}
class Doc {
  head = new Node(this);
  documentElement = { lang: "zh-CN" };
  nonce = "actual-page-nonce";
  querySelector() {
    return { nonce: this.nonce };
  }
  createElement() {
    return new Node(this);
  }
}
const leaf = (id: string, sessionId: string): PaneLeaf => ({
  type: "leaf",
  id,
  view: { kind: "terminal", sessionId },
});
function fixture(controls?: GutterControls) {
  const doc = new Doc(),
    root = new Node(doc),
    life = new AbortController(),
    failed = vi.fn(),
    handles = new Map<string, PaneHandle>();
  root.id = "workspace-terminal";
  const factory = vi.fn((n: PaneLeaf) => {
    const handle = {
      element: new Node(doc) as unknown as HTMLElement,
      mount: vi.fn(async () => {}),
      setVisible: vi.fn(async (_visible: boolean) => {}),
      dispose: vi.fn(),
    };
    handles.set(n.id, handle);
    return handle;
  });
  const view = new WorkspaceLayoutView(
    root as unknown as HTMLElement,
    life.signal,
    factory,
    failed,
    controls,
  );
  return { doc, root, life, failed, handles, factory, view };
}
it("nonce styles contain only validated ids/finite model ratios, never inline CSS", async () => {
  const f = fixture(),
    tree = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(tree, ["1", "2"]);
  const style = f.doc.head.children[0];
  expect(style?.attributes.get("nonce")).toBe("actual-page-nonce");
  expect(style?.textContent).toContain(
    "grid-template-columns:minmax(0,0.5fr) minmax(0,0.5fr)",
  );
  expect(f.root.children[0]?.attributes.has("style")).toBe(false);
  f.view.dispose();
  expect(f.doc.head.children).toHaveLength(0);
});
it("ratio changes reuse both actual leaf nodes and controllers, do not mount another PTY", async () => {
  const f = fixture(),
    root = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(root, ["1", "2"]);
  const a = f.handles.get("a"),
    b = f.handles.get("b");
  await f.view.render(setSplitSizes(root, "s", [3, 1]), ["1", "2"]);
  expect(f.factory).toHaveBeenCalledTimes(2);
  expect(a?.mount).toHaveBeenCalledOnce();
  expect(b?.mount).toHaveBeenCalledOnce();
  expect(f.root.children[0]?.children).toEqual([a?.element, b?.element]);
  f.view.dispose();
});
it("removing a layout leaf disposes only its display and preserves surviving controller", async () => {
  const f = fixture(),
    tree = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(tree, ["1", "2"]);
  const a = f.handles.get("a"),
    b = f.handles.get("b");
  await f.view.render(leaf("a", "1"), ["1", "2"]);
  expect(b?.dispose).toHaveBeenCalledOnce();
  expect(a?.dispose).not.toHaveBeenCalled();
  expect(f.root.children[0]).toBe(a?.element);
  f.view.dispose();
  expect(a?.dispose).toHaveBeenCalledOnce();
});
it("identity substitution and bad layouts do not mutate the accepted DOM", async () => {
  const f = fixture();
  await f.view.render(leaf("a", "1"), ["1", "2"]);
  const original = f.root.children[0];
  await expect(f.view.render(leaf("a", "2"), ["1", "2"])).rejects.toThrow(
    "identity_changed",
  );
  await expect(f.view.render(leaf("a", "3"), ["1", "2"])).rejects.toThrow(
    "layout_invalid",
  );
  expect(f.root.children[0]).toBe(original);
  expect(f.factory).toHaveBeenCalledOnce();
  f.view.dispose();
});
it("factory failure cleans partial new resources while retaining the old leaf", async () => {
  const f = fixture();
  await f.view.render(leaf("a", "1"), ["1", "2", "3"]);
  const a = f.handles.get("a"),
    create = f.factory.getMockImplementation();
  f.factory.mockImplementation((n) => {
    if (n.id === "c") throw Error("factory");
    if (!create) throw Error("fixture");
    return create(n);
  });
  const tree = splitPane(
    splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row"),
    "b",
    leaf("c", "3"),
    "s2",
    "column",
  );
  await expect(f.view.render(tree, ["1", "2", "3"])).rejects.toThrow("factory");
  expect(f.handles.get("b")?.dispose).toHaveBeenCalledOnce();
  expect(a?.dispose).not.toHaveBeenCalled();
  f.view.dispose();
});
it("abort detaches pending mounts; stale rejection cannot fail a new layout", async () => {
  const f = fixture();
  let reject!: (error: Error) => void;
  const create = f.factory.getMockImplementation();
  f.factory.mockImplementation((n) => {
    if (!create) throw Error("fixture");
    const handle = create(n);
    handle.mount.mockImplementation(
      () =>
        new Promise<void>((_resolve, r) => {
          reject = r;
        }),
    );
    return handle;
  });
  const job = f.view.render(leaf("a", "1"), ["1"]);
  f.life.abort();
  reject(Error("retired"));
  await job;
  expect(f.failed).not.toHaveBeenCalled();
  expect(f.handles.get("a")?.dispose).toHaveBeenCalledOnce();
  await expect(f.view.render(leaf("a", "1"), ["1"])).rejects.toThrow(
    "view.retired",
  );
});
it("missing nonce refuses layout before starting any controller", () => {
  const doc = new Doc();
  doc.nonce = "__TAURI_STYLE_NONCE__";
  const root = new Node(doc);
  root.id = "workspace-terminal";
  expect(
    () =>
      new WorkspaceLayoutView(
        root as unknown as HTMLElement,
        new AbortController().signal,
        vi.fn(),
        vi.fn(),
      ),
  ).toThrow("style_nonce_missing");
  expect(doc.head.children).toHaveLength(0);
  expect(decodePaneLayout(leaf("a", "1"), ["1"])).toEqual(leaf("a", "1"));
});
it("retired pending leaf cannot report a failure against its mounted replacement", async () => {
  const f = fixture(),
    create = f.factory.getMockImplementation();
  let reject!: (error: Error) => void;
  f.factory.mockImplementation((n) => {
    if (!create) throw Error("fixture");
    const handle = create(n);
    if (n.id === "a")
      handle.mount.mockImplementation(
        () =>
          new Promise<void>((_resolve, r) => {
            reject = r;
          }),
      );
    return handle;
  });
  const old = f.view.render(leaf("a", "1"), ["1", "2"]);
  await f.view.render(leaf("b", "2"), ["1", "2"]);
  reject(Error("old permission denied"));
  await old;
  expect(f.failed).not.toHaveBeenCalled();
  expect(f.handles.get("a")?.dispose).toHaveBeenCalledOnce();
  expect(f.root.children[0]).toBe(f.handles.get("b")?.element);
  f.view.dispose();
});

it("maximum view hides other leaves while preserving actual nodes and their controllers", async () => {
  const f = fixture(),
    tree = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(tree, ["1", "2"]);
  const a = f.handles.get("a"),
    b = f.handles.get("b");
  await f.view.render(tree, ["1", "2"], "1");
  expect(f.root.children[0]?.children[0]).toBe(a?.element);
  const hidden = f.root.children[0]?.children[1];
  expect(hidden?.attributes.has("hidden")).toBe(true);
  expect(hidden?.attributes.has("inert")).toBe(true);
  expect(hidden?.children).toEqual([b?.element]);
  expect(b?.setVisible).toHaveBeenLastCalledWith(false);
  expect(a?.setVisible).toHaveBeenLastCalledWith(true);
  await f.view.render(tree, ["1", "2"]);
  expect(f.factory).toHaveBeenCalledTimes(2);
  expect(b?.setVisible).toHaveBeenLastCalledWith(true);
  expect(a?.mount).toHaveBeenCalledOnce();
  expect(b?.mount).toHaveBeenCalledOnce();
  expect(b?.dispose).not.toHaveBeenCalled();
  f.view.dispose();
});

it("restored maximum defers zero-size hidden attachment until reveal and mounts each leaf once", async () => {
  const f = fixture(),
    tree = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(tree, ["1", "2"], "1");
  const a = f.handles.get("a"),
    b = f.handles.get("b");
  expect(a?.mount).toHaveBeenCalledOnce();
  expect(b?.mount).not.toHaveBeenCalled();
  expect(f.root.children[0]?.children[1]?.attributes.has("hidden")).toBe(true);
  await f.view.render(tree, ["1", "2"]);
  await f.view.render(tree, ["1", "2"], "2");
  expect(a?.mount).toHaveBeenCalledOnce();
  expect(b?.mount).toHaveBeenCalledOnce();
  expect(f.factory).toHaveBeenCalledTimes(2);
  f.view.dispose();
});

it("language switching updates a split separator without remounting panes or resizing", async () => {
  const controls = {
    enabled: () => true,
    resize: vi.fn(async () => {}),
    failed: vi.fn(),
  };
  const f = fixture(controls);
  const tree = splitPane(leaf("a", "1"), "a", leaf("b", "2"), "s", "row");
  await f.view.render(tree, ["1", "2"]);
  const layout = f.root.children[0];
  const gutter = layout?.children[1];
  expect(gutter?.attributes.get("aria-label")).toBe("调整分屏比例 s");
  applyUiLanguage(f.doc as unknown as Document, "en", f.life.signal);
  expect(gutter?.attributes.get("aria-label")).toBe("Adjust split ratio s");
  expect(gutter?.attributes.get("aria-valuenow")).toBe("50");
  expect(f.root.children[0]).toBe(layout);
  expect(f.factory).toHaveBeenCalledTimes(2);
  for (const handle of f.handles.values())
    expect(handle.mount).toHaveBeenCalledOnce();
  expect(controls.resize).not.toHaveBeenCalled();
  expect(controls.failed).not.toHaveBeenCalled();
  f.life.abort();
});
