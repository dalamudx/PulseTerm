import { expect, it, vi } from "vitest";
import { WorkspaceProfileManager } from "./workspace-profile-manager";

class Node {
  children: Node[] = [];
  listeners = new Map<string, (() => void)[]>();
  dataset: Record<string, string> = {};
  checked = false;
  disabled = false;
  open = false;
  textContent = "";
  type = "";
  value = "";
  className = "";
  ownerDocument = {
    createElement: () => new Node(),
    querySelector: () => null,
  };
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  addEventListener(key: string, fn: () => void) {
    this.listeners.set(key, [...(this.listeners.get(key) ?? []), fn]);
  }
  removeEventListener(key: string, fn: () => void) {
    this.listeners.set(
      key,
      (this.listeners.get(key) ?? []).filter((f) => f !== fn),
    );
  }
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
  }
  setAttribute() {}
  focus() {}
  querySelector() {
    return this.children.find((n) => n.type === "button");
  }
  emit(key: string) {
    for (const fn of this.listeners.get(key) ?? []) fn();
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const defaultOffer = {
  ticket,
  selected: "sh",
  available: true,
  offered: ["sh"],
};
function fixture() {
  const dialog = new Node(),
    list = new Node(),
    status = new Node(),
    defaults = new Node(),
    lifecycle = new Node(),
    life = new AbortController();
  const native = {
    deleted: vi.fn(async (): Promise<unknown> => []),
    prepare: vi.fn(async () => ({
      ticket,
      shell: "sh",
      action: "delete",
      isDefault: true,
    })),
    commit: vi.fn(async () => null),
    cancel: vi.fn(async () => null),
  };
  const defaultNative = {
    read: vi.fn(async (): Promise<unknown> => defaultOffer),
    save: vi.fn(async () => null),
    cancel: vi.fn(async () => null),
  };
  const save = vi.fn(async () => null),
    refresh = vi.fn(async () => [{ kind: "sh", hidden: false }] as const),
    launch = vi.fn(),
    working = vi.fn();
  const manager = new WorkspaceProfileManager({
    dialog: dialog as unknown as HTMLDialogElement,
    list: list as unknown as HTMLElement,
    status: status as unknown as HTMLElement,
    signal: life.signal,
    nonce: () => nonce,
    save,
    refresh,
    working,
    defaults: {
      root: defaults as unknown as HTMLElement,
      native: defaultNative,
      launch,
    },
    lifecycle: { root: lifecycle as unknown as HTMLElement, native },
  });
  manager.update([{ kind: "sh", hidden: false }]);
  return {
    manager,
    dialog,
    list,
    defaults,
    lifecycle,
    life,
    native,
    defaultNative,
    save,
    refresh,
    launch,
    working,
    remove: () => list.children[0]?.children[1],
    input: () => list.children[0]?.children[0]?.children[0],
    action: (n: number) => lifecycle.children[2]?.children[n],
  };
}
async function drain() {
  for (let i = 0; i < 30; i++) await Promise.resolve();
}
it("delete retires default ticket and visibility edits, then cancel serializes deleted/default rereads", async () => {
  const f = fixture();
  f.manager.open();
  await drain();
  const old = f.remove();
  old?.emit("click");
  await drain();
  expect(f.defaultNative.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.defaults.children).toHaveLength(0);
  expect(f.input()?.disabled).toBe(true);
  old?.emit("click");
  f.input()?.emit("change");
  expect(f.native.prepare).toHaveBeenCalledOnce();
  expect(f.save).not.toHaveBeenCalled();
  expect(f.launch).not.toHaveBeenCalled();
  let finish!: (v: unknown) => void;
  f.native.deleted.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const readCount = f.defaultNative.read.mock.calls.length;
  f.action(0)?.emit("click");
  await drain();
  expect(f.defaultNative.read).toHaveBeenCalledTimes(readCount);
  finish([]);
  await drain();
  expect(f.defaultNative.read).toHaveBeenCalledTimes(readCount + 1);
  expect(f.native.commit).not.toHaveBeenCalled();
  f.manager.dispose();
});
it("close/reopen retires late default read before it can start old deleted read", async () => {
  const f = fixture();
  let resolve!: (v: unknown) => void;
  f.defaultNative.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  f.manager.open();
  f.manager.close();
  f.manager.open();
  await drain();
  expect(f.native.deleted).toHaveBeenCalledOnce();
  const current = f.lifecycle.children;
  resolve(defaultOffer);
  await drain();
  expect(f.native.deleted).toHaveBeenCalledOnce();
  expect(f.lifecycle.children).toBe(current);
  f.manager.dispose();
});
it("page retirement during confirmation cancels and never launches or refreshes", async () => {
  const f = fixture();
  f.manager.open();
  await drain();
  f.remove()?.emit("click");
  await drain();
  const confirm = f.action(1);
  f.life.abort();
  confirm?.emit("click");
  await drain();
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.native.commit).not.toHaveBeenCalled();
  expect(f.refresh).not.toHaveBeenCalled();
  expect(f.launch).not.toHaveBeenCalled();
  expect(f.dialog.open).toBe(false);
});
