import { expect, it, vi } from "vitest";
import { workspaceKeybindingCommandIds } from "./workspace-command-catalog";
import { decodeWorkspaceKeymap } from "./workspace-keymap";
import { WorkspaceKeymapEditor } from "./workspace-keymap-editor";

class Node extends EventTarget {
  children: Node[] = [];
  parent: Node | null = null;
  textContent = "";
  value = "";
  className = "";
  type = "";
  checked = false;
  disabled = false;
  open = false;
  isConnected = true;
  rows = 0;
  maxLength = 0;
  spellcheck = false;
  placeholder = "";
  dataset: Record<string, string> = {};
  constructor(
    readonly ownerDocument: Doc,
    readonly tag = "div",
  ) {
    super();
  }
  append(...nodes: Node[]) {
    for (const node of nodes) {
      node.parent = this;
      node.isConnected = this.isConnected;
      this.children.push(node);
    }
  }
  replaceChildren(...nodes: Node[]) {
    for (const node of this.children) node.disconnect();
    this.children = [];
    this.append(...nodes);
  }
  disconnect() {
    this.isConnected = false;
    for (const child of this.children) child.disconnect();
  }
  setAttribute(_name: string, _value: string) {}
  querySelectorAll(selector: string): Node[] {
    const nodes = this.children.flatMap((node) => [
      node,
      ...node.querySelectorAll("*"),
    ]);
    return selector === "*"
      ? nodes
      : nodes.filter((node) =>
          selector === "[data-rewrite]"
            ? !!node.dataset.rewrite
            : selector.split(",").includes(node.tag),
        );
  }
  querySelector(selector: string): Node | null {
    return this.querySelectorAll(selector)[0] ?? null;
  }
  showModal() {
    this.open = true;
    this.ownerDocument.modal = this;
  }
  close() {
    this.open = false;
    this.ownerDocument.modal = null;
  }
  focus() {
    this.ownerDocument.activeElement = this;
  }
}
class Doc {
  activeElement: Node | null = null;
  modal: Node | null = null;
  createElement(tag: string) {
    return new Node(this, tag);
  }
  createTextNode(text: string) {
    const node = new Node(this, "text");
    node.textContent = text;
    return node;
  }
  querySelector(_selector: string) {
    return this.modal;
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
function frame(confirmed: boolean, id = 1) {
  return decodeWorkspaceKeymap({
    version: 1,
    token: `${String(id).repeat(8)}-1111-4111-8111-111111111111`,
    confirmed,
    values: {
      version: 1,
      bindings: [
        {
          action: "find",
          keys: "ctrl+shift+f",
          when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
        },
        {
          action: "find",
          keys: "meta+shift+f",
          when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
        },
        {
          action: "palette",
          keys: "ctrl+shift+p",
          when: "workspaceFocus && !editableFocus && !modalOpen",
        },
        {
          action: "palette",
          keys: "meta+shift+p",
          when: "workspaceFocus && !editableFocus && !modalOpen",
        },
      ],
      sources: [...workspaceKeybindingCommandIds, "palette"].map((action) => ({
        action,
        source: "default",
        disabled: action !== "palette" && action !== "find",
      })),
    },
  });
}
function fixture() {
  const doc = new Doc(),
    dialog = new Node(doc, "dialog"),
    fields = new Node(doc),
    status = new Node(doc),
    opener = new Node(doc, "button"),
    life = new AbortController();
  doc.activeElement = opener;
  let current = nonce;
  const changed = vi.fn(),
    invalidate = vi.fn(),
    working = vi.fn();
  const deps = {
    read: vi.fn(async () => frame(false)),
    confirm: vi.fn(async () => frame(true, 2)),
    save: vi.fn(async () => frame(false, 3)),
    cancel: vi.fn(async () => {}),
  };
  const editor = new WorkspaceKeymapEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => current,
      changed,
      invalidate,
      working,
    },
    deps,
  );
  return {
    editor,
    deps,
    doc,
    dialog,
    fields,
    status,
    life,
    opener,
    changed,
    invalidate,
    working,
    setNonce: (value: string) => {
      current = value;
    },
  };
}
async function flush() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}
function node(f: ReturnType<typeof fixture>, tag: string) {
  const result = f.fields.querySelector(tag);
  if (!result) throw Error("fixture node missing");
  return result;
}
it("opens one bounded form only after current-page source confirmation", async () => {
  const f = fixture();
  await f.editor.open();
  expect(f.deps.read).toHaveBeenCalledOnce();
  expect(f.deps.confirm).toHaveBeenCalledOnce();
  expect(f.changed).toHaveBeenCalledOnce();
  expect(f.fields.querySelectorAll("select")[0]?.children).toHaveLength(9);
  expect(node(f, "textarea").value).toContain("ctrl+shift+p");
  expect(f.invalidate).toHaveBeenCalledOnce();
  f.editor.dispose();
});
it("saving requires explicit canonical consent, and editing clears it", async () => {
  const f = fixture();
  await f.editor.open();
  const consent = node(f, "[data-rewrite]");
  const save = f.fields.querySelectorAll("button")[0];
  expect(save?.disabled).toBe(true);
  consent.checked = true;
  consent.dispatchEvent(new Event("change"));
  expect(save?.disabled).toBe(false);
  node(f, "textarea").dispatchEvent(new Event("input"));
  expect(consent.checked).toBe(false);
  expect(save?.disabled).toBe(true);
  expect(f.deps.save).not.toHaveBeenCalled();
  f.editor.dispose();
});
it("old replaced form cannot reuse a new action or source ticket", async () => {
  const f = fixture();
  await f.editor.open();
  const old = node(f, "form"),
    consent = node(f, "[data-rewrite]");
  consent.checked = true;
  consent.dispatchEvent(new Event("change"));
  const select = node(f, "select");
  select.value = "settings";
  select.dispatchEvent(new Event("change"));
  old.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.deps.save).not.toHaveBeenCalled();
  f.editor.dispose();
});
it("confirmed save is re-confirmed before notifying binding installation", async () => {
  const f = fixture();
  await f.editor.open();
  const consent = node(f, "[data-rewrite]");
  consent.checked = true;
  consent.dispatchEvent(new Event("change"));
  node(f, "form").dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  expect(f.deps.save).toHaveBeenCalledOnce();
  expect(f.deps.confirm).toHaveBeenCalledTimes(2);
  expect(f.changed).toHaveBeenCalledTimes(2);
  expect(f.status.textContent).toContain("保存并确认");
  f.editor.dispose();
});
it("closing a pending read aborts without an extra duplicate cancel", async () => {
  const f = fixture();
  let signal: AbortSignal | undefined;
  f.deps.read.mockImplementation(async (_nonce?: unknown, value?: unknown) => {
    signal = value as AbortSignal;
    return new Promise(() => {});
  });
  void f.editor.open();
  f.editor.close();
  expect(signal?.aborted).toBe(true);
  expect(f.deps.cancel).not.toHaveBeenCalled();
  expect(f.changed).not.toHaveBeenCalled();
  f.editor.dispose();
});
it("late read from an old opening cannot confirm or draw into a reopened form", async () => {
  const f = fixture();
  let release: (value: ReturnType<typeof frame>) => void = () => {};
  f.deps.read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        release = resolve;
      }),
  );
  const old = f.editor.open();
  f.editor.close();
  await f.editor.open();
  release(frame(false));
  await old;
  expect(f.deps.confirm).toHaveBeenCalledOnce();
  expect(f.changed).toHaveBeenCalledOnce();
  expect(f.dialog.open).toBe(true);
  f.editor.dispose();
});
it("save uncertainty invalidates tickets and never repeats or enables bindings", async () => {
  const f = fixture();
  await f.editor.open();
  f.deps.save.mockRejectedValueOnce(Error("uncertain"));
  const consent = node(f, "[data-rewrite]");
  consent.checked = true;
  consent.dispatchEvent(new Event("change"));
  node(f, "form").dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  expect(f.deps.save).toHaveBeenCalledOnce();
  expect(f.changed).toHaveBeenCalledOnce();
  expect(f.fields.children).toHaveLength(0);
  expect(f.status.textContent).toContain("不自动重试");
  f.editor.dispose();
});
it("idle close revokes original-page ticket and restores only current connected opener", async () => {
  const f = fixture();
  await f.editor.open();
  f.editor.close();
  expect(f.deps.cancel).toHaveBeenCalledExactlyOnceWith(nonce);
  expect(f.doc.activeElement).toBe(f.opener);
  await f.editor.open();
  f.setNonce("bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb");
  f.doc.activeElement = null;
  f.editor.close();
  expect(f.deps.cancel).toHaveBeenLastCalledWith(nonce);
  expect(f.doc.activeElement).toBeNull();
  f.editor.dispose();
});
