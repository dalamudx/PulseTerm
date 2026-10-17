import { expect, it, vi } from "vitest";
import { WorkspaceProfileManager } from "./workspace-profile-manager";

class TestNode {
  children: TestNode[] = [];
  listeners = new Map<string, EventListener[]>();
  dataset: Record<string, string> = {};
  checked = false;
  disabled = false;
  open = false;
  textContent = "";
  type = "";
  ownerDocument: { createElement: () => TestNode; querySelector: () => null };
  constructor() {
    this.ownerDocument = {
      createElement: () => new TestNode(),
      querySelector: () => null,
    };
  }
  replaceChildren() {
    this.children = [];
  }
  append(...nodes: TestNode[]) {
    this.children.push(...nodes);
  }
  addEventListener(key: string, handler: EventListener) {
    this.listeners.set(key, [...(this.listeners.get(key) ?? []), handler]);
  }
  removeEventListener(key: string, handler: EventListener) {
    this.listeners.set(
      key,
      (this.listeners.get(key) ?? []).filter((h) => h !== handler),
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
    return undefined;
  }
  change(checked: boolean) {
    this.checked = checked;
    for (const h of this.listeners.get("change") ?? []) h({} as Event);
  }
}
function fixture() {
  const dialog = new TestNode(),
    list = new TestNode(),
    status = new TestNode(),
    life = new AbortController(),
    save = vi.fn(async () => null),
    refresh = vi.fn(async () => [{ kind: "sh", hidden: true }] as const),
    working = vi.fn();
  const manager = new WorkspaceProfileManager({
    dialog: dialog as unknown as HTMLDialogElement,
    list: list as unknown as HTMLElement,
    status: status as unknown as HTMLElement,
    signal: life.signal,
    nonce: () => "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    save,
    refresh,
    working,
  });
  manager.update([{ kind: "sh", hidden: false }]);
  manager.open();
  return {
    dialog,
    list,
    status,
    life,
    save,
    refresh,
    working,
    manager,
    input: () => list.children[0]?.children[0],
  };
}
async function drain() {
  for (let i = 0; i < 8; i++) await Promise.resolve();
}
it("one checkbox explicit visibility request, busy suppresses another and fresh stored values redraw", async () => {
  const f = fixture(),
    old = f.input();
  old?.change(false);
  old?.change(false);
  expect(f.save).toHaveBeenCalledExactlyOnceWith(
    "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
    "sh",
    true,
  );
  expect(f.input()?.disabled).toBe(true);
  await drain();
  expect(f.refresh).toHaveBeenCalledOnce();
  expect(f.input()?.checked).toBe(false);
  expect(f.input()?.disabled).toBe(false);
  f.manager.dispose();
});
it("stale rendered input never applies to a newer catalog or closed dialog", async () => {
  const f = fixture(),
    old = f.input();
  f.manager.update([{ kind: "sh", hidden: true }]);
  old?.change(false);
  expect(f.save).not.toHaveBeenCalled();
  const current = f.input();
  f.manager.close();
  current?.change(true);
  expect(f.save).not.toHaveBeenCalled();
  f.manager.dispose();
});
it("old page result cannot refresh or clear a replacement owner's busy state", async () => {
  const f = fixture();
  let resolve!: (value: null) => void;
  f.save.mockImplementation(
    () =>
      new Promise<null>((r) => {
        resolve = r;
      }),
  );
  f.input()?.change(false);
  f.life.abort();
  expect(f.dialog.open).toBe(false);
  expect(f.list.children).toHaveLength(0);
  resolve(null);
  await drain();
  expect(f.refresh).not.toHaveBeenCalled();
  expect(f.working).toHaveBeenCalledExactlyOnceWith(true);
});
it("uncertain native response does not publish optimistic preferences, rollback or auto retry", async () => {
  const f = fixture();
  f.save.mockRejectedValue(Error("uncertain"));
  f.input()?.change(false);
  await drain();
  expect(f.refresh).not.toHaveBeenCalled();
  expect(f.save).toHaveBeenCalledOnce();
  expect(f.input()?.checked).toBe(true);
  expect(f.dialog.open).toBe(false);
  expect(f.status.textContent).toContain("不自动重试");
  f.manager.dispose();
});
