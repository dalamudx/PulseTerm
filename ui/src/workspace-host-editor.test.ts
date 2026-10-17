import { afterEach, expect, it, vi } from "vitest";
import { WorkspaceHostEditor } from "./workspace-host-editor";
import { decodeHostCatalogue, type HostCatalogue } from "./workspace-hosts";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  type = "";
  className = "";
  disabled = false;
  open = false;
  get ownerDocument() {
    return document;
  }
  constructor(readonly tag = "div") {
    super();
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  querySelectorAll(): Node[] {
    return this.children
      .flatMap((n) => [n, ...n.querySelectorAll()])
      .filter(
        (n) => n.tag === "input" || n.tag === "button" || n.tag === "select",
      );
  }
  setAttribute(_name: string, _value: string): void {}
  child(index: number): Node {
    const node = this.children[index];
    if (!node) throw Error("fixture.child_missing");
    return node;
  }
  focus(): void {}
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  id = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa";
const value = () =>
  decodeHostCatalogue({
    version: 1,
    token: nonce,
    revision: "1",
    hosts: [
      { id, label: "<img src=x>", host: "host", port: 22, username: "user" },
    ],
  });
afterEach(() => vi.unstubAllGlobals());
function fixture(catalogue: HostCatalogue = value()) {
  vi.stubGlobal("document", { createElement: (tag: string) => new Node(tag) });
  const dialog = new Node("dialog"),
    fields = new Node(),
    status = new Node(),
    life = new AbortController(),
    working = vi.fn();
  const dependencies = {
    read: vi.fn(async () => catalogue),
    save: vi.fn(async () => catalogue),
  };
  const editor = new WorkspaceHostEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      done: new Node("button") as unknown as HTMLButtonElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      signal: life.signal,
      nonce: () => nonce,
      working,
    },
    dependencies,
  );
  return { editor, dialog, fields, status, life, dependencies, working };
}
const machineId = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
function graphValue(migrated = true) {
  return decodeHostCatalogue({
    version: 2,
    token: nonce,
    revision: "1",
    migrated,
    machines: migrated ? [{ id: machineId, label: "Machine" }] : [],
    hosts: [
      {
        id,
        label: "Primary",
        host: "first.invalid",
        port: 22,
        username: "user",
        machineId: migrated ? machineId : null,
      },
    ],
  });
}
it("explicit migration and attaching a second entrance use the original single save ticket", async () => {
  const legacy = fixture(graphValue(false));
  await legacy.editor.open();
  legacy.fields.child(2).dispatchEvent(new Event("click"));
  await Promise.resolve();
  expect(legacy.dependencies.save).toHaveBeenCalledWith(
    nonce,
    nonce,
    { action: "migrate_catalogue" },
    expect.any(AbortSignal),
  );
  legacy.editor.close();
  const f = fixture(graphValue());
  await f.editor.open();
  const select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  const form = f.fields.child(1);
  form.child(0).child(0).value = "Secondary";
  form.child(1).child(0).value = "second.invalid";
  form.child(3).child(0).value = "other";
  form.child(4).child(0).value = machineId;
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).toHaveBeenCalledWith(
    nonce,
    nonce,
    {
      action: "attach",
      machineId,
      label: "Secondary",
      host: "second.invalid",
      port: 22,
      username: "other",
    },
    expect.any(AbortSignal),
  );
});
it("replaced machine forms and foreign parent selections cannot save", async () => {
  const f = fixture(graphValue());
  await f.editor.open();
  const old = f.fields.child(2),
    select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  old.dispatchEvent(new Event("submit", { cancelable: true }));
  const form = f.fields.child(1);
  form.child(0).child(0).value = "Other";
  form.child(1).child(0).value = "other.invalid";
  form.child(3).child(0).value = "user";
  form.child(4).child(0).value = "cccccccc-cccc-7ccc-8ccc-cccccccccccc";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).not.toHaveBeenCalled();
});
it("renders literal metadata with no executable markup, closing invalidates old forms", async () => {
  const f = fixture();
  await f.editor.open();
  expect(f.fields.child(0).child(0).child(1).textContent).toBe("<img src=x>");
  const form = f.fields.child(1);
  f.editor.close();
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.fields.children).toHaveLength(0);
});
it("late read cannot replace reopened editor", async () => {
  const f = fixture();
  let resolve!: (v: HostCatalogue) => void;
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const pending = f.editor.open();
  f.editor.close();
  await f.editor.open();
  resolve(
    decodeHostCatalogue({ version: 1, token: nonce, revision: "0", hosts: [] }),
  );
  await pending;
  expect(f.fields.children).toHaveLength(3);
});
it("deletion requires two clicks and failed save cannot reuse capture or retry", async () => {
  const f = fixture();
  f.dependencies.save.mockRejectedValueOnce(Error("workspace.hosts_conflict"));
  await f.editor.open();
  const form = f.fields.child(1),
    actions = form.child(4),
    remove = actions.child(1);
  remove.dispatchEvent(new Event("click"));
  expect(f.dependencies.save).not.toHaveBeenCalled();
  remove.dispatchEvent(new Event("click"));
  await Promise.resolve();
  await Promise.resolve();
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  expect(f.status.textContent).toContain("不自动重试");
  remove.dispatchEvent(new Event("click"));
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
});
it("page abort discards late committed-save receipt without rolling back native write", async () => {
  const f = fixture();
  let resolve!: (v: HostCatalogue) => void;
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await f.editor.open();
  const remove = f.fields.child(1).child(4).child(1);
  remove.dispatchEvent(new Event("click"));
  remove.dispatchEvent(new Event("click"));
  f.life.abort();
  resolve(value());
  await Promise.resolve();
  await Promise.resolve();
  expect(f.dialog.open).toBe(false);
  expect(f.fields.children).toHaveLength(0);
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
});

it("selection replacement retires detached same-capture forms without consuming the ticket", async () => {
  const f = fixture();
  await f.editor.open();
  const oldForm = f.fields.child(1),
    select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  expect(f.fields.child(1).child(0).child(0).value).toBe("");
  oldForm.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(
    f.fields
      .child(1)
      .querySelectorAll()
      .filter((n) => n.tag === "input"),
  ).toHaveLength(4);
});
