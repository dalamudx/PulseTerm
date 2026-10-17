import { expect, it, vi } from "vitest";
import type { WorkspaceCommand } from "./workspace-command-catalog";
import { WorkspaceCommandPalette } from "./workspace-command-palette";

class Node extends EventTarget {
  id = "";
  value = "";
  textContent = "";
  open = false;
  isConnected = true;
  focuses = 0;
  children: Node[] = [];
  attrs = new Map<string, string>();
  ownerDocument: {
    activeElement: Node | null;
    createElement: (tag: string) => Node;
  };
  constructor(
    readonly tag = "div",
    doc?: Node["ownerDocument"],
  ) {
    super();
    this.ownerDocument = doc ?? {
      activeElement: null,
      createElement: (tag) => new Node(tag, this.ownerDocument),
    };
  }
  setAttribute(name: string, value: string) {
    this.attrs.set(name, value);
  }
  removeAttribute(name: string) {
    this.attrs.delete(name);
  }
  append(node: Node) {
    this.children.push(node);
  }
  replaceChildren() {
    this.children = [];
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  focus() {
    this.focuses++;
    this.ownerDocument.activeElement = this;
  }
  scrollIntoView() {}
}
function key(node: Node, value: string, extra: object = {}) {
  const event = new Event("keydown", { cancelable: true });
  Object.assign(event, {
    key: value,
    repeat: false,
    isComposing: false,
    ...extra,
  });
  node.dispatchEvent(event);
  return event;
}
function fixture() {
  const origin = new Node("button"),
    doc = origin.ownerDocument;
  doc.activeElement = origin;
  const dialog = new Node("dialog", doc),
    search = new Node("input", doc),
    list = new Node("ul", doc),
    status = new Node("p", doc),
    done = new Node("button", doc);
  const life = new AbortController();
  let nonce = "page-one",
    available = true,
    target = "session-one";
  const settings = vi.fn(() => expect(dialog.open).toBe(false)),
    close = vi.fn();
  const commands: readonly WorkspaceCommand[] = [
    {
      id: "settings",
      title: "终端设置",
      keywords: "font",
      available: () => true,
      target: () => "",
      run: settings,
    },
    {
      id: "close-session",
      title: "关闭当前会话",
      keywords: "close",
      available: () => true,
      target: () => target,
      run: close,
    },
  ];
  const palette = new WorkspaceCommandPalette({
    dialog: dialog as unknown as HTMLDialogElement,
    search: search as unknown as HTMLInputElement,
    list: list as unknown as HTMLElement,
    status: status as unknown as HTMLElement,
    done: done as unknown as HTMLButtonElement,
    context: {
      signal: life.signal,
      nonce: () => nonce,
      available: () => available,
    },
    commands,
  });
  return {
    palette,
    origin,
    dialog,
    search,
    list,
    status,
    done,
    life,
    settings,
    close,
    setNonce: (v: string) => {
      nonce = v;
    },
    setAvailable: (v: boolean) => {
      available = v;
    },
    setTarget: (v: string) => {
      target = v;
    },
  };
}
it("IME candidate confirmation and navigation do not execute commands, even without per-key flags", () => {
  const f = fixture();
  expect(f.palette.open()).toBe(true);
  f.search.dispatchEvent(new Event("compositionstart"));
  for (const value of ["ArrowDown", "Enter", "Escape"])
    expect(key(f.search, value).defaultPrevented).toBe(false);
  expect(f.settings).not.toHaveBeenCalled();
  expect(f.close).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(true);
  f.search.dispatchEvent(new Event("compositionend"));
  expect(key(f.search, "Enter", { keyCode: 229 }).defaultPrevented).toBe(false);
  expect(f.settings).not.toHaveBeenCalled();
  key(f.search, "Enter");
  expect(f.settings).toHaveBeenCalledOnce();
  f.life.abort();
});
it("opens search with accessible options and executes selected existing action only after closing", () => {
  const f = fixture();
  expect(f.palette.open()).toBe(true);
  expect(f.search.focuses).toBe(1);
  expect(f.list.children).toHaveLength(2);
  expect(f.search.attrs.get("aria-activedescendant")).toBe(
    "workspace-command-settings",
  );
  key(f.search, "Enter");
  expect(f.settings).toHaveBeenCalledOnce();
  expect(f.dialog.open).toBe(false);
  expect(f.search.value).toBe("");
});
it("search and direction keys select literal current metadata", () => {
  const f = fixture();
  f.palette.open();
  key(f.search, "ArrowDown");
  expect(f.search.attrs.get("aria-activedescendant")).toBe(
    "workspace-command-close-session",
  );
  key(f.search, "ArrowUp");
  f.search.value = "close";
  f.search.dispatchEvent(new Event("input"));
  expect(f.list.children).toHaveLength(1);
  key(f.search, "Enter");
  expect(f.close).toHaveBeenCalledOnce();
  expect(f.settings).not.toHaveBeenCalled();
});
it("empty matches never execute and composition/repeated Enter do not invoke commands", () => {
  const f = fixture();
  f.palette.open();
  key(f.search, "Enter", { isComposing: true });
  key(f.search, "Enter", { repeat: true });
  f.search.value = "missing";
  f.search.dispatchEvent(new Event("input"));
  key(f.search, "Enter");
  expect(f.status.textContent).toBe("没有匹配的可用命令");
  expect(f.search.attrs.has("aria-activedescendant")).toBe(false);
  expect(f.settings).not.toHaveBeenCalled();
});
it("native cancel restores only current-page connected opener", () => {
  const f = fixture();
  f.palette.open();
  f.dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
  expect(f.origin.focuses).toBe(1);
  expect(f.dialog.open).toBe(false);
  expect(f.settings).not.toHaveBeenCalled();
  f.palette.open();
  f.setNonce("page-two");
  f.palette.close();
  expect(f.origin.focuses).toBe(1);
});
it("page exit cleans dialog/search/options without focusing an old target", () => {
  const f = fixture();
  f.palette.open();
  f.life.abort();
  expect(f.dialog.open).toBe(false);
  expect(f.list.children).toHaveLength(0);
  expect(f.origin.focuses).toBe(0);
  expect(f.palette.open()).toBe(false);
});
it("busy/fit or nonce changes invalidate a visible palette", () => {
  const f = fixture();
  f.palette.open();
  f.setAvailable(false);
  f.palette.check();
  expect(f.dialog.open).toBe(false);
  expect(f.origin.focuses).toBe(0);
  expect(f.palette.open()).toBe(false);
  f.setAvailable(true);
  f.palette.open();
  f.setNonce("new-page");
  f.palette.check();
  expect(f.dialog.open).toBe(false);
});
it("changing focused session cannot execute old close choice", () => {
  const f = fixture();
  f.palette.open();
  key(f.search, "ArrowDown");
  f.setTarget("session-two");
  key(f.search, "Enter");
  expect(f.close).not.toHaveBeenCalled();
});
it("queued option clicks from a previous opening cannot execute a new capture", () => {
  const f = fixture();
  f.palette.open();
  const old = f.list.children[0];
  f.palette.close();
  f.palette.open();
  old?.dispatchEvent(new Event("click"));
  expect(f.settings).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(true);
});
it("queued close events from a previous opening do not close the new modal", () => {
  const f = fixture();
  f.palette.open();
  f.palette.close();
  f.palette.open();
  f.dialog.dispatchEvent(new Event("close"));
  expect(f.dialog.open).toBe(true);
});
it("a replaced filtered option cannot execute a no-longer-shown command", () => {
  const f = fixture();
  f.palette.open();
  const old = f.list.children[0];
  f.search.value = "close";
  f.search.dispatchEvent(new Event("input"));
  old?.dispatchEvent(new Event("click"));
  expect(f.settings).not.toHaveBeenCalled();
});
it("shortcut captures Ctrl/MetaShiftP including repeats, but not normal CtrlP or composition", () => {
  const f = fixture();
  const event = (extra: object = {}) =>
    ({
      key: "P",
      ctrlKey: true,
      metaKey: false,
      shiftKey: true,
      altKey: false,
      repeat: false,
      isComposing: false,
      defaultPrevented: false,
      preventDefault: vi.fn(),
      stopImmediatePropagation: vi.fn(),
      ...extra,
    }) as unknown as KeyboardEvent;
  const first = event();
  expect(f.palette.shortcut(first)).toBe(true);
  expect(first.preventDefault).toHaveBeenCalled();
  const repeat = event({ repeat: true });
  expect(f.palette.shortcut(repeat)).toBe(true);
  expect(repeat.stopImmediatePropagation).toHaveBeenCalled();
  expect(f.search.focuses).toBe(1);
  expect(f.palette.shortcut(event({ shiftKey: false }))).toBe(false);
  expect(f.palette.shortcut(event({ isComposing: true }))).toBe(false);
  expect(f.palette.shortcut(event({ keyCode: 229 }))).toBe(false);
});
