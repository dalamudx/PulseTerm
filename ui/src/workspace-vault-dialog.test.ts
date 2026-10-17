import { afterEach, expect, it, vi } from "vitest";
import type { VaultStatus } from "./workspace-vault";
import { WorkspaceVaultDialog } from "./workspace-vault-dialog";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  id = "";
  type = "";
  className = "";
  dataset: Record<string, string> = {};
  disabled = false;
  open = false;
  get ownerDocument() {
    return document;
  }
  focus = vi.fn();
  constructor(readonly tag = "div") {
    super();
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  all(): Node[] {
    return this.children.flatMap((n) => [n, ...n.all()]);
  }
  querySelectorAll(selector: string) {
    return this.all().filter((n) => selector.split(",").includes(n.tag));
  }
  setAttribute(_name: string, _value: string) {}
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  find(id: string) {
    const node = this.all().find((n) => n.id === id);
    if (!node) throw Error("fixture.node");
    return node;
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const status = (state: VaultStatus["state"]): VaultStatus => ({
  version: 1,
  state,
});
afterEach(() => {
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
});
function fixture(initial: VaultStatus["state"] = "missing") {
  vi.stubGlobal("document", { createElement: (tag: string) => new Node(tag) });
  const dialog = new Node("dialog"),
    fields = new Node(),
    label = new Node(),
    done = new Node("button"),
    life = new AbortController(),
    working = vi.fn();
  let current: string | undefined = nonce;
  const dependencies = {
    read: vi.fn(async (_n: string, _s: AbortSignal) => status(initial)),
    lock: vi.fn(async (_n: string, _s: AbortSignal) => status("locked")),
    password: vi.fn(
      async (
        _kind: "create" | "unlock",
        _n: string,
        _b: Uint8Array,
        _s: AbortSignal,
      ) => status("locked"),
    ),
  };
  const editor = new WorkspaceVaultDialog(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: label as unknown as HTMLElement,
      done: done as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => current,
      working,
    },
    dependencies,
  );
  return {
    editor,
    fields,
    label,
    dialog,
    done,
    life,
    dependencies,
    working,
    nonce(value: string | undefined) {
      current = value;
    },
  };
}
const settle = async () => {
  for (let n = 0; n < 8; n++) await Promise.resolve();
};
function fill(f: ReturnType<typeof fixture>, text = "fixed-dialog-password") {
  f.fields.find("workspace-vault-password").value = text;
  const repeat = f.fields.all().find((n) => n.id === "workspace-vault-repeat");
  if (repeat) repeat.value = text;
  const form = f.fields.all().find((n) => n.tag === "form");
  if (!form) throw Error("fixture.form");
  return form;
}
it("close publishes available controls only after the modal is closed", async () => {
  const f = fixture("locked");
  await f.editor.open();
  let observed: boolean | undefined;
  f.working.mockImplementation((value: boolean) => {
    if (!value) observed = f.dialog.open;
  });
  f.editor.close();
  expect(observed).toBe(false);
});
it("source failure never constructs fallback vault/password form", async () => {
  const f = fixture();
  f.dependencies.read.mockRejectedValue(Error("private-not-rendered"));
  await f.editor.open();
  expect(f.label.dataset.state).toBe("error");
  expect(f.fields.all().filter((n) => n.tag === "input")).toHaveLength(0);
  expect(f.dependencies.password).not.toHaveBeenCalled();
  expect(f.label.textContent).not.toContain("private-not-rendered");
});
it("creation masks/clears inputs and validates exact confirmation before dispatch", async () => {
  const f = fixture();
  await f.editor.open();
  const form = fill(f);
  expect(f.fields.find("workspace-vault-password").type).toBe("password");
  f.fields.find("workspace-vault-repeat").value = "different";
  form.dispatchEvent(new Event("submit"));
  expect(f.dependencies.password).not.toHaveBeenCalled();
  expect(f.fields.find("workspace-vault-password").value).toBe("");
});
it("actual create receipt remains locked and owned buffers are cleared", async () => {
  const f = fixture();
  await f.editor.open();
  fill(f).dispatchEvent(new Event("submit"));
  await settle();
  expect(f.label.dataset.state).toBe("locked");
  const bytes = f.dependencies.password.mock.calls[0]?.[2];
  if (!bytes) throw Error("fixture.bytes");
  expect([...bytes].every((b) => b === 0)).toBe(true);
  expect(f.fields.find("workspace-vault-password").value).toBe("");
});
it("close clears own bytes immediately even when native/provider ignores cancellation", async () => {
  const f = fixture();
  let resolve: (v: VaultStatus) => void = () => {};
  f.dependencies.password.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await f.editor.open();
  const form = fill(f);
  form.dispatchEvent(new Event("submit"));
  const args = f.dependencies.password.mock.calls[0];
  if (!args) throw Error("fixture.work");
  f.editor.close();
  expect(args[3].aborted).toBe(true);
  expect([...args[2]].every((b) => b === 0)).toBe(true);
  expect(
    form
      .all()
      .filter((n) => n.tag === "input")
      .every((n) => n.value === ""),
  ).toBe(true);
  resolve(status("available"));
  await settle();
  expect(f.dialog.open).toBe(false);
  expect(f.fields.children).toHaveLength(0);
});
it("old form and old lock button cannot dispatch into a replacement rendering", async () => {
  const f = fixture();
  await f.editor.open();
  const form = fill(f);
  f.editor.close();
  f.dependencies.read.mockResolvedValue(status("available"));
  await f.editor.open();
  form.dispatchEvent(new Event("submit"));
  expect(f.dependencies.password).not.toHaveBeenCalled();
  const lock = f.fields.find("workspace-vault-lock");
  f.editor.close();
  f.dependencies.read.mockResolvedValue(status("locked"));
  await f.editor.open();
  lock.dispatchEvent(new Event("click"));
  expect(f.dependencies.lock).not.toHaveBeenCalled();
});
it("missing nonce after encoding still wipes buffers without native dispatch", async () => {
  const f = fixture();
  await f.editor.open();
  const owned = new Uint8Array([1, 2, 3]);
  vi.spyOn(TextEncoder.prototype, "encode").mockReturnValue(owned);
  const form = fill(f);
  f.nonce(undefined);
  form.dispatchEvent(new Event("submit"));
  await settle();
  expect([...owned]).toEqual([0, 0, 0]);
  expect(f.dependencies.password).not.toHaveBeenCalled();
});
it("failed unlock requires fresh source read, clears password and never auto retries", async () => {
  const f = fixture("locked");
  f.dependencies.password.mockRejectedValue(
    Error("private-password-do-not-render"),
  );
  await f.editor.open();
  fill(f).dispatchEvent(new Event("submit"));
  await settle();
  expect(f.label.dataset.state).toBe("error");
  expect(f.fields.all().filter((n) => n.tag === "input")).toHaveLength(0);
  expect(f.fields.find("workspace-vault-read")).toBeDefined();
  expect(f.label.textContent).not.toContain("private-password-do-not-render");
  expect(f.dependencies.password).toHaveBeenCalledTimes(1);
});
it("page retirement rejects late results and focus without closing sessions", async () => {
  const f = fixture();
  let resolve: (v: VaultStatus) => void = () => {};
  f.dependencies.read.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const opened = f.editor.open();
  f.life.abort();
  resolve(status("available"));
  await opened;
  expect(f.dialog.open).toBe(false);
  expect(f.fields.children).toHaveLength(0);
  expect(f.done.focus).not.toHaveBeenCalled();
});
