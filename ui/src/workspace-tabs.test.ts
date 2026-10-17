import { expect, it, vi } from "vitest";
import {
  preferredWorkspaceSession,
  rememberWorkspaceSession,
  WorkspaceTabs,
} from "./workspace-tabs";

it("refresh restores only a current catalog identity, never a stale or forged authority", () => {
  const getItem = vi.fn(() => "18446744073709551615");
  expect(
    preferredWorkspaceSession(["1", "18446744073709551615"], { getItem }),
  ).toBe("18446744073709551615");
  getItem.mockReturnValue("2");
  expect(preferredWorkspaceSession(["1"], { getItem })).toBe("1");
  expect(preferredWorkspaceSession([], { getItem })).toBeUndefined();
});
it("unavailable display storage never blocks a valid catalog or creates a session", () => {
  expect(
    preferredWorkspaceSession(["3"], {
      getItem() {
        throw Error("disabled");
      },
    }),
  ).toBe("3");
  expect(() =>
    rememberWorkspaceSession("3", {
      setItem() {
        throw Error("disabled");
      },
    }),
  ).not.toThrow();
});
it("remembering a selection rejects noncanonical ids before touching storage", () => {
  const setItem = vi.fn();
  for (const id of ["0", "01", "<script>", "18446744073709551616"])
    expect(() => rememberWorkspaceSession(id, { setItem })).toThrow();
  expect(setItem).not.toHaveBeenCalled();
  rememberWorkspaceSession("5", { setItem });
  expect(setItem).toHaveBeenCalledExactlyOnceWith(
    "pulseterm.workspace.selected.v1",
    "5",
  );
});
class TestDoc {
  activeElement: TestNode | null = null;
  createElement() {
    return new TestNode(this);
  }
}
class TestNode {
  children: TestNode[] = [];
  attributes = new Map<string, string>();
  dataset: Record<string, string> = {};
  listeners = new Map<string, EventListener[]>();
  id = "";
  type = "";
  className = "";
  textContent = "";
  disabled = false;
  tabIndex = -1;
  constructor(readonly ownerDocument: TestDoc) {}
  setAttribute(name: string, value: string) {
    this.attributes.set(name, value);
  }
  contains(node: TestNode | null) {
    return node === this || this.children.includes(node as TestNode);
  }
  replaceChildren(...children: TestNode[]) {
    this.children = children;
  }
  append(node: TestNode) {
    this.children.push(node);
  }
  addEventListener(name: string, callback: EventListener) {
    const handlers = this.listeners.get(name) ?? [];
    handlers.push(callback);
    this.listeners.set(name, handlers);
  }
  querySelector(selector: string) {
    return this.children.find((node) => `#${node.id}` === selector);
  }
  querySelectorAll() {
    return this.children;
  }
  focus() {
    if (!this.disabled) this.ownerDocument.activeElement = this;
  }
  click() {
    for (const callback of this.listeners.get("click") ?? [])
      callback({} as Event);
  }
  press(key: string, mods: Partial<KeyboardEvent> = {}) {
    const preventDefault = vi.fn();
    const event = { key, preventDefault, ...mods };
    for (const callback of this.listeners.get("keydown") ?? [])
      callback(event as unknown as Event);
    return preventDefault;
  }
}
function fixture() {
  const doc = new TestDoc(),
    bar = new TestNode(doc),
    panel = new TestNode(doc),
    select = vi.fn();
  panel.id = "workspace-terminal";
  const tabs = new WorkspaceTabs(
    bar as unknown as HTMLElement,
    panel as unknown as HTMLElement,
    select,
  );
  return { doc, bar, panel, select, tabs };
}
it("declares manual tabs and panel ownership, not a second active terminal", () => {
  const f = fixture();
  f.tabs.render(["1", "2"], "2", false);
  expect(f.bar.children.map((n) => n.attributes.get("aria-selected"))).toEqual([
    "false",
    "true",
  ]);
  expect(f.bar.children.map((n) => n.tabIndex)).toEqual([-1, 0]);
  expect(f.panel.attributes.get("aria-labelledby")).toBe("workspace-tab-2");
  for (const n of f.bar.children) {
    expect(n.attributes.get("role")).toBe("tab");
    expect(n.attributes.get("aria-controls")).toBe("workspace-terminal");
  }
});
it("arrows wrap and Home/End move only focus; explicit activation switches", () => {
  const f = fixture();
  f.tabs.render(["1", "2", "3"], "1", false);
  f.bar.children[0]?.press("ArrowLeft");
  expect(f.doc.activeElement?.dataset.session).toBe("3");
  f.doc.activeElement?.press("Home");
  expect(f.doc.activeElement?.dataset.session).toBe("1");
  f.doc.activeElement?.press("End");
  expect(f.doc.activeElement?.dataset.session).toBe("3");
  expect(f.select).not.toHaveBeenCalled();
  f.doc.activeElement?.click();
  expect(f.select).toHaveBeenCalledExactlyOnceWith("3");
});
it("modifiers and native Enter/Space are not hijacked", () => {
  const f = fixture();
  f.tabs.render(["1", "2"], "1", false);
  const button = f.bar.children[0];
  for (const key of ["Enter", " ", "Tab", "r"])
    expect(button?.press(key)).not.toHaveBeenCalled();
  for (const mods of [{ ctrlKey: true }, { metaKey: true }, { altKey: true }])
    expect(button?.press("ArrowRight", mods)).not.toHaveBeenCalled();
  expect(f.select).not.toHaveBeenCalled();
});
it("busy state and stale render nodes cannot activate or move new focus", () => {
  const f = fixture();
  f.tabs.render(["1", "2"], "1", false);
  const old = f.bar.children[1];
  f.tabs.render(["1", "2"], "1", true);
  old?.click();
  old?.press("End");
  f.bar.children[1]?.click();
  expect(f.select).not.toHaveBeenCalled();
  expect(f.bar.children.every((n) => n.disabled)).toBe(true);
  f.tabs.render(["1", "2"], "1", false);
  old?.click();
  expect(f.select).not.toHaveBeenCalled();
});
it("removing focused tab leaves one focusable survivor, empty resets panel label", () => {
  const f = fixture();
  f.tabs.render(["1", "2"], "2", false);
  f.bar.children[1]?.focus();
  f.tabs.render(["1"], "1", false);
  expect(f.bar.children[0]?.tabIndex).toBe(0);
  expect(f.panel.attributes.get("aria-labelledby")).toBe("workspace-tab-1");
  f.tabs.render([], undefined, false);
  expect(f.bar.children).toHaveLength(0);
  expect(f.panel.attributes.get("aria-labelledby")).toBe("workspace-title");
});
