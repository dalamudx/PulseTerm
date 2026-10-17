import { afterEach, expect, it, vi } from "vitest";
import {
  decodeIdentityCatalogue,
  type IdentityCatalogue,
} from "./workspace-identities";
import { WorkspaceIdentityEditor } from "./workspace-identity-editor";
import type { saveIdentityPassword } from "./workspace-identity-password";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  type = "";
  className = "";
  disabled = false;
  open = false;
  focus = vi.fn();
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
      .filter((n) => ["input", "button", "select"].includes(n.tag));
  }
  setAttribute(_key: string, _value: string): void {}
  child(i: number): Node {
    const node = this.children[i];
    if (!node) throw Error("fixture.node");
    return node;
  }
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
  decodeIdentityCatalogue({
    version: 1,
    token: nonce,
    revision: "1",
    vaultPresent: true,
    identities: [
      {
        id,
        label: "<img src=x>",
        username: "user",
        secret: { id: nonce, kind: "ssh_password", hasValue: false },
      },
    ],
  });
it("password string is cleared on visibility reset/selection and detached generation cannot submit", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.child(2),
    input = form.child(0).child(0);
  input.value = "must-clear";
  form.child(1).child(0).dispatchEvent(new Event("click"));
  expect(input.type).toBe("text");
  const select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  expect(input.value).toBe("");
  expect(input.type).toBe("password");
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.writePassword).not.toHaveBeenCalled();
});
it("unsaved metadata cannot route a password to a misleading new username", async () => {
  const f = fixture();
  await f.editor.open();
  f.fields.child(1).child(1).child(0).value = "different";
  const form = f.fields.child(2),
    input = form.child(0).child(0);
  input.value = "typed";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(input.value).toBe("");
  expect(f.status.textContent).toContain("先保存");
  expect(f.writePassword).not.toHaveBeenCalled();
});
it("closing pending save clears owned bytes/cancels scope and late receipt cannot read redraw or focus", async () => {
  const f = fixture();
  let finish: () => void = () => {};
  f.writePassword.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.editor.open();
  const form = f.fields.child(2),
    input = form.child(0).child(0);
  input.value = "fixed-owned-password";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(input.value).toBe("");
  const call = f.writePassword.mock.calls[0];
  if (!call) throw Error("fixture.password");
  f.editor.close();
  expect(call[3].aborted).toBe(true);
  finish();
  await Promise.resolve();
  await Promise.resolve();
  expect(call[2].every((byte) => byte === 0)).toBe(true);
  expect(f.dependencies.read).toHaveBeenCalledTimes(1);
  expect(f.done.focus).not.toHaveBeenCalled();
  expect(f.fields.children).toHaveLength(0);
});
it("confirmed write rereads metadata without secret reflection and restores Done focus", async () => {
  const f = fixture();
  await f.editor.open();
  const next = decodeIdentityCatalogue({
    version: 1,
    token: nonce,
    revision: "2",
    vaultPresent: true,
    identities: [
      {
        id,
        label: "saved",
        username: "user",
        secret: { id: nonce, kind: "ssh_password", hasValue: true },
      },
    ],
  });
  f.dependencies.read.mockResolvedValueOnce(next);
  const form = f.fields.child(2),
    input = form.child(0).child(0);
  input.value = "fixed-write";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.dependencies.read).toHaveBeenCalledTimes(2);
  expect(f.done.focus).toHaveBeenCalledOnce();
  expect(f.status.textContent).toContain("不授予");
  expect(f.fields.child(2).child(0).child(0).value).toBe("");
});
it("current rejected write restores Done after removing focused fields, so cancel remains keyboard reachable", async () => {
  const f = fixture();
  f.writePassword.mockRejectedValueOnce(Error("fixed-denied"));
  await f.editor.open();
  const form = f.fields.child(2);
  form.child(0).child(0).value = "fixed";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  expect(f.done.focus).toHaveBeenCalledOnce();
  expect(f.fields.children).toHaveLength(0);
  f.dialog.dispatchEvent(new Event("cancel", { cancelable: true }));
  expect(f.dialog.open).toBe(false);
});
afterEach(() => vi.unstubAllGlobals());
function fixture() {
  vi.stubGlobal("document", { createElement: (tag: string) => new Node(tag) });
  const dialog = new Node("dialog"),
    fields = new Node(),
    status = new Node(),
    done = new Node("button"),
    life = new AbortController(),
    working = vi.fn();
  const dependencies = {
    read: vi.fn(async () => value()),
    save: vi.fn(async () => value()),
  };
  const writePassword = vi.fn<typeof saveIdentityPassword>(async () => {});
  const editor = new WorkspaceIdentityEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      done: done as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => nonce,
      working,
    },
    dependencies,
    writePassword,
  );
  return {
    editor,
    dialog,
    fields,
    status,
    done,
    life,
    dependencies,
    writePassword,
  };
}
it("absent Vault presents a truthful empty state without input or save", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValueOnce(
    decodeIdentityCatalogue({
      version: 1,
      token: nonce,
      revision: "0",
      vaultPresent: false,
      identities: [],
    }),
  );
  await f.editor.open();
  expect(f.fields.querySelectorAll()).toHaveLength(0);
  expect(f.status.textContent).toContain("不会自动生成密钥");
  expect(f.dependencies.save).not.toHaveBeenCalled();
});
it("literal label and readonly existence render two metadata inputs and empty receive-only password", async () => {
  const f = fixture();
  await f.editor.open();
  expect(f.fields.child(0).child(0).child(1).textContent).toBe("<img src=x>");
  const inputs = f.fields
    .querySelectorAll()
    .filter(
      (n) => n.tag === "input" && n.type !== "search" && n.type !== "checkbox",
    );
  expect(inputs).toHaveLength(3);
  expect(inputs.filter((n) => n.type === "text")).toHaveLength(2);
  expect(inputs.filter((n) => n.type === "password")[0]?.value).toBe("");
  expect(f.fields.child(1).child(2).textContent).toContain("未设置");
  const form = f.fields.child(1);
  f.editor.close();
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).not.toHaveBeenCalled();
});
it("detached same-capture form cannot consume ticket after selector replacement", async () => {
  const f = fixture();
  await f.editor.open();
  const old = f.fields.child(1),
    select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  old.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(f.fields.child(1).child(0).child(0).value).toBe("");
});
it("late read cannot replace reopened current editor", async () => {
  const f = fixture();
  let finish: (value: IdentityCatalogue) => void = () => {};
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const pending = f.editor.open();
  f.editor.close();
  await f.editor.open();
  finish(
    decodeIdentityCatalogue({
      version: 1,
      token: nonce,
      revision: "0",
      vaultPresent: false,
      identities: [],
    }),
  );
  await pending;
  expect(f.fields.children).toHaveLength(4);
});
it("double deletion confirmation and uncertain save invalidate fields without retry", async () => {
  const f = fixture();
  f.dependencies.save.mockRejectedValueOnce(
    Error("workspace.identities_conflict"),
  );
  await f.editor.open();
  const remove = f.fields.child(1).child(3).child(1);
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
it("page abort blocks late save drawing/focus without rolling back native commit", async () => {
  const f = fixture();
  let finish: (value: IdentityCatalogue) => void = () => {};
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  await f.editor.open();
  const remove = f.fields.child(1).child(3).child(1);
  remove.dispatchEvent(new Event("click"));
  remove.dispatchEvent(new Event("click"));
  f.life.abort();
  finish(value());
  await Promise.resolve();
  await Promise.resolve();
  expect(f.fields.children).toHaveLength(0);
  expect(f.dialog.open).toBe(false);
  expect(f.done.focus).not.toHaveBeenCalled();
});
it("successful metadata save focuses Done with empty receive-only credential input", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.child(1);
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await Promise.resolve();
  await Promise.resolve();
  expect(f.done.focus).toHaveBeenCalledTimes(1);
  expect(
    f.fields
      .querySelectorAll()
      .filter(
        (n) =>
          n.tag === "input" && n.type !== "search" && n.type !== "checkbox",
      ),
  ).toHaveLength(3);
  expect(f.fields.child(2).child(0).child(0).value).toBe("");
});
