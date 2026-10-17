import { afterEach, expect, it, vi } from "vitest";
import {
  decodeKeyIdentityCatalogue,
  type KeyIdentityCatalogue,
  type KeyIdentityChange,
} from "./workspace-key-identities";
import { WorkspaceKeyIdentityEditor } from "./workspace-key-identity-editor";

class Node extends EventTarget {
  children: Node[] = [];
  textContent = "";
  value = "";
  type = "";
  className = "";
  disabled = false;
  open = false;
  focus = vi.fn();
  attributes = new Map<string, string>();
  set ariaLabel(value: string) {
    this.setAttribute("aria-label", value);
  }
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
      .flatMap((node) => [node, ...node.querySelectorAll()])
      .filter((node) =>
        ["input", "textarea", "button", "select"].includes(node.tag),
      );
  }
  setAttribute(key: string, value: string) {
    this.attributes.set(key, value);
  }
  child(index: number): Node {
    const node = this.children[index];
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
  id = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
  pass = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const value = (): KeyIdentityCatalogue =>
  decodeKeyIdentityCatalogue({
    version: 1,
    token: nonce,
    revision: "1",
    vaultPresent: true,
    identities: [
      {
        id,
        label: "<img src=x>",
        username: "user",
        secret: { id: nonce, kind: "ssh_private_key", hasValue: false },
        passphrase: { id: pass, kind: "ssh_key_passphrase", hasValue: false },
      },
    ],
  });
function fixture(
  imports?: import("./workspace-key-identity-editor").KeyImportSupport,
) {
  vi.stubGlobal("document", { createElement: (tag: string) => new Node(tag) });
  const dialog = new Node("dialog"),
    fields = new Node(),
    status = new Node(),
    done = new Node("button"),
    life = new AbortController(),
    state = { nonce: nonce as string | undefined };
  const dependencies = {
    read: vi.fn(async (_nonce: string, _signal: AbortSignal) => value()),
    save: vi.fn(
      async (
        _nonce: string,
        _token: string,
        _change: KeyIdentityChange,
        _signal: AbortSignal,
      ) => value(),
    ),
  };
  const working = vi.fn();
  const editor = new WorkspaceKeyIdentityEditor(
    {
      dialog: dialog as unknown as HTMLDialogElement,
      fields: fields as unknown as HTMLElement,
      status: status as unknown as HTMLElement,
      done: done as unknown as HTMLButtonElement,
      signal: life.signal,
      nonce: () => state.nonce,
      working,
      imports,
    },
    dependencies,
  );
  return {
    editor,
    dialog,
    fields,
    status,
    done,
    life,
    state,
    dependencies,
    working,
  };
}
async function flush() {
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
}
afterEach(() => vi.unstubAllGlobals());
it("metadata-only initial state renders malicious-looking names as literal text and no credential fields", async () => {
  const f = fixture();
  await f.editor.open();
  expect(f.fields.child(0).child(0).child(1).textContent).toBe("<img src=x>");
  expect(f.fields.child(1).child(2).textContent).toBe(
    "私钥：未保存 · 解密口令：未保存",
  );
  expect(
    f.fields
      .querySelectorAll()
      .filter(
        (node) =>
          node.tag === "input" &&
          node.type !== "search" &&
          node.type !== "checkbox",
      )
      .map((node) => node.type),
  ).toEqual(["text", "text"]);
  expect(f.fields.child(0).child(0).attributes.get("aria-label")).toBe(
    "编辑私钥身份",
  );
});
it("missing Vault shows a disabled creation state without synthesizing an identity", async () => {
  const f = fixture();
  f.dependencies.read.mockResolvedValueOnce(
    decodeKeyIdentityCatalogue({
      version: 1,
      token: nonce,
      revision: "0",
      vaultPresent: false,
      identities: [],
    }),
  );
  await f.editor.open();
  expect(f.fields.querySelectorAll()).toHaveLength(0);
  expect(f.status.textContent).toContain("先创建");
  expect(f.dependencies.save).not.toHaveBeenCalled();
});
it("unconfirmed page never reads or exposes a form", async () => {
  const f = fixture();
  f.state.nonce = undefined;
  await f.editor.open();
  expect(f.dependencies.read).not.toHaveBeenCalled();
  expect(f.fields.children).toHaveLength(0);
  expect(f.status.textContent).toContain("尚未就绪");
});
it("close cancels the actual read and late success cannot redraw a reopened generation", async () => {
  const f = fixture();
  let finish: (value: KeyIdentityCatalogue) => void = () => {};
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const old = f.editor.open();
  const signal = f.dependencies.read.mock.calls[0]?.[1];
  f.editor.close();
  expect(signal?.aborted).toBe(true);
  await f.editor.open();
  const rendered = f.fields.child(1);
  finish(value());
  await old;
  expect(f.fields.child(1)).toBe(rendered);
  expect(f.done.focus).not.toHaveBeenCalled();
});
it("nonce replacement cannot publish old metadata even with unchanged DOM and epoch", async () => {
  const f = fixture();
  let finish: (value: KeyIdentityCatalogue) => void = () => {};
  f.dependencies.read.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const pending = f.editor.open();
  f.state.nonce = pass;
  finish(value());
  await pending;
  expect(f.fields.children).toHaveLength(0);
  expect(f.done.focus).not.toHaveBeenCalled();
  f.editor.close();
});
it("detached form cannot save or arm a deletion after selection changes", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.child(1),
    remove = form.child(3).child(1);
  const select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  remove.dispatchEvent(new Event("click"));
  remove.dispatchEvent(new Event("click"));
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).not.toHaveBeenCalled();
});
it("creation sends only name username and passphrase policy, leaving refs backend-selected", async () => {
  const f = fixture();
  await f.editor.open();
  const select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  const form = f.fields.child(1);
  form.child(0).child(0).value = "my-key";
  form.child(1).child(0).value = "user";
  form.child(2).child(0).value = "yes";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  expect(f.dependencies.save).toHaveBeenCalledWith(
    nonce,
    nonce,
    {
      action: "create",
      label: "my-key",
      username: "user",
      withPassphrase: true,
    },
    expect.any(AbortSignal),
  );
  expect(f.done.focus).toHaveBeenCalledTimes(1);
});
it("delete requires its own second click and frozen metadata snapshot", async () => {
  const f = fixture();
  await f.editor.open();
  const remove = f.fields.child(1).child(3).child(1);
  remove.dispatchEvent(new Event("click"));
  expect(f.dependencies.save).not.toHaveBeenCalled();
  expect(remove.textContent).toBe("确认删除身份");
  remove.dispatchEvent(new Event("click"));
  await flush();
  expect(f.dependencies.save.mock.calls[0]?.[2]).toEqual({
    action: "remove",
    id,
  });
});
it("busy duplicate submit cannot start another mutation; closed late success cannot focus or redraw", async () => {
  const f = fixture();
  let finish: (value: KeyIdentityCatalogue) => void = () => {};
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.editor.open();
  const form = f.fields.child(1);
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  expect(form.child(0).child(0).disabled).toBe(true);
  const signal = f.dependencies.save.mock.calls[0]?.[3];
  f.editor.close();
  expect(signal?.aborted).toBe(true);
  finish(value());
  await flush();
  expect(f.fields.children).toHaveLength(0);
  expect(f.done.focus).not.toHaveBeenCalled();
});
it("abort disposes the original page and late save cannot become a new page success", async () => {
  const f = fixture();
  let finish: (value: KeyIdentityCatalogue) => void = () => {};
  f.dependencies.save.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.editor.open();
  f.fields.child(1).dispatchEvent(new Event("submit", { cancelable: true }));
  f.life.abort();
  expect(f.dependencies.save.mock.calls[0]?.[3].aborted).toBe(true);
  finish(value());
  await flush();
  await f.editor.open();
  expect(f.dialog.open).toBe(false);
  expect(f.dependencies.read).toHaveBeenCalledTimes(1);
  expect(f.done.focus).not.toHaveBeenCalled();
});
it("save rejection clears uncertain fields and never retries, retaining a recoverable close button", async () => {
  const f = fixture();
  f.dependencies.save.mockRejectedValueOnce(Error("fixed.conflict"));
  await f.editor.open();
  f.fields.child(1).dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  expect(f.fields.children).toHaveLength(0);
  expect(f.status.textContent).toContain("关闭后重新打开");
  expect(f.dependencies.save).toHaveBeenCalledTimes(1);
  expect(f.done.focus).toHaveBeenCalledTimes(1);
  expect(f.working).toHaveBeenLastCalledWith(false);
});
it("validation rejects option usernames and forged passphrase policy before RPC", async () => {
  const f = fixture();
  await f.editor.open();
  const form = f.fields.child(1);
  form.child(1).child(0).value = "-option";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).not.toHaveBeenCalled();
  const select = f.fields.child(0).child(0);
  select.value = "";
  select.dispatchEvent(new Event("change"));
  const create = f.fields.child(1);
  create.child(0).child(0).value = "name";
  create.child(1).child(0).value = "user";
  create.child(2).child(0).value = "forged";
  create.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(f.dependencies.save).not.toHaveBeenCalled();
});

const imports = () => ({
  available: vi.fn(async (_nonce: string, _signal: AbortSignal) => true),
  save: vi.fn(
    async (
      _nonce: string,
      _identity: string,
      _key: Uint8Array,
      _pass: Uint8Array | undefined,
      _signal: AbortSignal,
    ) => {},
  ),
});
it("import form requires explicit backend availability and masks passphrase", async () => {
  const support = imports(),
    f = fixture(support);
  await f.editor.open();
  const form = f.fields.child(2);
  expect(form.child(1).child(0).tag).toBe("textarea");
  expect(form.child(2).child(0).type).toBe("password");
  expect(support.available).toHaveBeenCalledWith(
    nonce,
    expect.any(AbortSignal),
  );
  expect(support.save).not.toHaveBeenCalled();
});
it("empty or oversized private input stays disabled after load and metadata selection unlock", async () => {
  const f = fixture(imports());
  await f.editor.open();
  const form = f.fields.child(2),
    key = form.child(1).child(0),
    pass = form.child(2).child(0),
    button = form.child(3).child(0);
  expect(button.disabled).toBe(true);
  key.value = "fixed-key";
  key.dispatchEvent(new Event("input"));
  expect(button.disabled).toBe(true);
  pass.value = "fixed-pass";
  pass.dispatchEvent(new Event("input"));
  expect(button.disabled).toBe(false);
  key.value = "界".repeat(21846);
  key.dispatchEvent(new Event("input"));
  expect(button.disabled).toBe(true);
  key.value = "fixed-key";
  pass.value = "界".repeat(1366);
  pass.dispatchEvent(new Event("input"));
  expect(button.disabled).toBe(true);
  const select = f.fields.child(0).child(0);
  select.value = id;
  select.dispatchEvent(new Event("change"));
  expect(f.fields.child(2).child(3).child(0).disabled).toBe(true);
  f.editor.dispose();
});
it("default or failed availability leaves metadata usable and has no secret fields", async () => {
  for (const fail of [false, true]) {
    const support = imports();
    if (fail) support.available.mockRejectedValue(Error("unavailable"));
    else support.available.mockResolvedValue(false);
    const f = fixture(support);
    await f.editor.open();
    expect(f.fields.children).toHaveLength(3);
    expect(f.fields.child(1).child(0).child(0).disabled).toBe(false);
    f.editor.dispose();
  }
});
it("saving owns exact UTF8 input, clears visible fields immediately and suppresses double submit", async () => {
  const support = imports(),
    f = fixture(support);
  let finish: () => void = () => {};
  support.save.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.editor.open();
  const form = f.fields.child(2),
    key = form.child(1).child(0),
    pass = form.child(2).child(0);
  key.value = "fixed-private-key\n";
  pass.value = "fixed-passphrase";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(key.value).toBe("");
  expect(pass.value).toBe("");
  expect(key.disabled).toBe(true);
  expect(support.save).toHaveBeenCalledTimes(1);
  const call = support.save.mock.calls[0];
  if (!call) throw Error("fixture.call");
  expect(call[0]).toBe(nonce);
  expect(call[1]).toBe(id);
  expect(new TextDecoder().decode(call[2])).toBe("fixed-private-key\n");
  expect(new TextDecoder().decode(call[3])).toBe("fixed-passphrase");
  finish();
  await flush();
  await flush();
  expect(call[2].every((byte) => byte === 0)).toBe(true);
  expect(call[3]?.every((byte) => byte === 0)).toBe(true);
  expect(f.status.textContent).toBe("私钥已保存，输入已清除");
});
it("closing or changing selection clears both input controls and forbids old form", async () => {
  for (const close of [false, true]) {
    const support = imports(),
      f = fixture(support);
    await f.editor.open();
    const old = f.fields.child(2),
      key = old.child(1).child(0),
      pass = old.child(2).child(0);
    key.value = "fixed-key";
    pass.value = "fixed-pass";
    if (close) f.editor.close();
    else {
      const select = f.fields.child(0).child(0);
      select.value = "";
      select.dispatchEvent(new Event("change"));
    }
    expect(key.value).toBe("");
    expect(pass.value).toBe("");
    old.dispatchEvent(new Event("submit", { cancelable: true }));
    expect(support.save).not.toHaveBeenCalled();
    f.editor.dispose();
  }
});
it("late availability after dialog closes cannot restore credentials or enable buttons", async () => {
  const support = imports(),
    f = fixture(support);
  let finish: (enabled: boolean) => void = () => {};
  support.available.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const opened = f.editor.open();
  f.editor.close();
  finish(true);
  await opened;
  expect(f.fields.children).toHaveLength(0);
  expect(support.save).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});
it("late write after close cannot display success or start metadata reload", async () => {
  const support = imports(),
    f = fixture(support);
  let finish: () => void = () => {};
  support.save.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.editor.open();
  const form = f.fields.child(2);
  form.child(1).child(0).value = "fixed-key";
  form.child(2).child(0).value = "fixed-pass";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  const call = support.save.mock.calls[0];
  if (!call) throw Error("fixture.call");
  f.editor.close();
  expect(call[4].aborted).toBe(true);
  const status = f.status.textContent;
  finish();
  await flush();
  await flush();
  expect(f.fields.children).toHaveLength(0);
  expect(f.status.textContent).toBe(status);
  expect(f.dependencies.read).toHaveBeenCalledTimes(1);
  expect(call[2].every((byte) => byte === 0)).toBe(true);
});
it("uncertain or failed write removes stale metadata tickets and clears all input without retry", async () => {
  const support = imports(),
    f = fixture(support);
  support.save.mockRejectedValue(Error("identity.key_uncertain"));
  await f.editor.open();
  const form = f.fields.child(2);
  form.child(1).child(0).value = "fixed-key";
  form.child(2).child(0).value = "fixed-pass";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await flush();
  await flush();
  expect(f.fields.children).toHaveLength(0);
  expect(support.save).toHaveBeenCalledTimes(1);
  expect(f.status.textContent).toContain("保存未确认");
  expect(f.done.focus).toHaveBeenCalled();
});
