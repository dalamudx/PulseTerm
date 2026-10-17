import { afterEach, expect, it, vi } from "vitest";
import {
  CatalogueBatch,
  validateCatalogueIds,
} from "./workspace-catalogue-batch";
import { validateHostChange } from "./workspace-hosts";
import { validateIdentityChange } from "./workspace-identities";
import { validateKeyIdentityChange } from "./workspace-key-identities";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  tag: string;
  type = "";
  checked = false;
  indeterminate = false;
  hidden = false;
  disabled = false;
  get ownerDocument() {
    return document;
  }
  constructor(tag = "div") {
    super();
    this.tag = tag;
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  setAttribute(): void {}
  focus(): void {}
  querySelectorAll(): Node[] {
    return this.children.flatMap((n) => [n, ...n.querySelectorAll()]);
  }
  child(i: number): Node {
    const n = this.children[i];
    if (!n) throw Error("test.node");
    return n;
  }
}
const id = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
  second = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
afterEach(() => vi.unstubAllGlobals());
function fixture() {
  vi.stubGlobal("document", { createElement: (tag: string) => new Node(tag) });
  const container = new Node(),
    state = { current: true },
    remove = vi.fn(),
    edit = vi.fn(),
    filter = vi.fn();
  const batch = new CatalogueBatch({
    container: container as unknown as HTMLElement,
    rows: [
      { id, label: "<script>", description: "user@one" },
      { id: second, label: "Second", description: "user@two" },
    ],
    kind: "SSH 主机",
    consequence: "现有会话继续运行",
    current: () => state.current,
    remove,
    edit,
    filter,
  });
  const root = container.child(0),
    search = root.child(1).child(0),
    all = root.child(2).child(0),
    list = root.child(3),
    button = root.child(4),
    confirm = root.child(5);
  return {
    batch,
    state,
    remove,
    edit,
    filter,
    search,
    all,
    list,
    button,
    confirm,
  };
}
it("all three wire adapters validate nonempty unique bounded IDs and one metadata action", () => {
  for (const ids of [[], [id, id], ["raw-path"], Array(257).fill(id)]) {
    expect(() => validateCatalogueIds(ids)).toThrow();
    for (const validate of [
      validateHostChange,
      validateIdentityChange,
      validateKeyIdentityChange,
    ])
      expect(() => validate({ action: "remove_many", ids })).toThrow();
  }
  const ids = validateCatalogueIds([id, second]);
  expect(Object.isFrozen(ids)).toBe(true);
  expect(validateHostChange({ action: "remove_many", ids })).toEqual({
    action: "remove_many",
    ids,
  });
  for (const validate of [validateIdentityChange, validateKeyIdentityChange])
    expect(validate({ action: "remove_many", ids })).toEqual({
      operation: "remove_many",
      ids,
    });
  expect(() =>
    validateHostChange({ action: "remove_many", ids, command: "exec" }),
  ).toThrow();
});
it("filter retires hidden selections and their detached confirmation, and Cancel writes nothing", () => {
  const f = fixture();
  f.all.checked = true;
  f.all.dispatchEvent(new Event("change"));
  f.button.dispatchEvent(new Event("click"));
  const stale = f.confirm.child(3);
  expect(f.confirm.child(1).child(0).textContent).toContain("<script>");
  f.confirm.child(2).dispatchEvent(new Event("click"));
  stale.dispatchEvent(new Event("click"));
  expect(f.remove).not.toHaveBeenCalled();
  f.button.dispatchEvent(new Event("click"));
  const old = f.confirm.child(3);
  f.search.value = "Second";
  f.search.dispatchEvent(new Event("input"));
  expect(f.filter).toHaveBeenCalledOnce();
  old.dispatchEvent(new Event("click"));
  expect(f.remove).not.toHaveBeenCalled();
  expect(f.button.disabled).toBe(true);
  f.all.checked = true;
  f.all.dispatchEvent(new Event("change"));
  f.button.dispatchEvent(new Event("click"));
  old.dispatchEvent(new Event("click"));
  stale.dispatchEvent(new Event("click"));
  expect(f.remove).not.toHaveBeenCalled();
  f.confirm.child(3).dispatchEvent(new Event("click"));
  f.confirm.child(3).dispatchEvent(new Event("click"));
  expect(f.remove).toHaveBeenCalledExactlyOnceWith([second]);
});
it("busy and retired page callbacks cannot change selection or submit, and edits use original IDs", () => {
  const f = fixture();
  f.batch.setBusy(true);
  f.all.checked = true;
  f.all.dispatchEvent(new Event("change"));
  expect(f.button.disabled).toBe(true);
  f.batch.setBusy(false);
  f.list.child(0).child(1).dispatchEvent(new Event("click"));
  expect(f.edit).toHaveBeenCalledExactlyOnceWith(id);
  f.all.checked = true;
  f.all.dispatchEvent(new Event("change"));
  f.button.dispatchEvent(new Event("click"));
  f.state.current = false;
  f.confirm.child(3).dispatchEvent(new Event("click"));
  expect(f.remove).not.toHaveBeenCalled();
});
