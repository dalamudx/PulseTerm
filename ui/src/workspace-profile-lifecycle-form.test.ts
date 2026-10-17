import { afterEach, expect, it, vi } from "vitest";
import { ProfileLifecycleForm } from "./workspace-profile-lifecycle-form";

class Node {
  children: Node[] = [];
  textContent = "";
  disabled = false;
  type = "";
  className = "";
  focused = false;
  listeners = new Map<string, (() => void)[]>();
  ownerDocument = { createElement: () => new Node() };
  append(...nodes: Node[]) {
    this.children.push(...nodes);
  }
  replaceChildren(...nodes: Node[]) {
    this.children = nodes;
  }
  setAttribute() {}
  addEventListener(event: string, fn: () => void) {
    this.listeners.set(event, [...(this.listeners.get(event) ?? []), fn]);
  }
  querySelector() {
    return this.children.find((n) => n.type === "button");
  }
  focus() {
    this.focused = true;
  }
  click() {
    for (const fn of this.listeners.get("click") ?? []) fn();
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const offer = { ticket, shell: "sh", action: "delete", isDefault: true };
function fixture() {
  const root = new Node(),
    life = new AbortController();
  let page = nonce;
  const native = {
    deleted: vi.fn(async (): Promise<unknown> => []),
    prepare: vi.fn(async (): Promise<unknown> => offer),
    commit: vi.fn(async (): Promise<unknown> => null),
    cancel: vi.fn(async () => null),
  };
  const engaged = vi.fn(),
    changed = vi.fn(async () => {});
  const form = new ProfileLifecycleForm({
    root: root as unknown as HTMLElement,
    signal: life.signal,
    nonce: () => page,
    native,
    engaged,
    changed,
  });
  return {
    root,
    life,
    native,
    engaged,
    changed,
    form,
    page: (v: string) => {
      page = v;
    },
    status: () => root.children[1]?.textContent,
    button: (n: number) => root.children[2]?.children[n],
  };
}
async function drain() {
  for (let i = 0; i < 20; i++) await Promise.resolve();
}
afterEach(() => vi.useRealTimers());
it("delete confirmation explains default/session effects and defaults focus to cancel", async () => {
  const f = fixture();
  await f.form.prepare("sh", "delete");
  expect(f.root.children[0]?.textContent).toBe("删除 POSIX sh");
  expect(f.status()).toContain("默认启动不可用");
  expect(f.status()).toContain("已有终端继续运行");
  expect(f.button(0)?.focused).toBe(true);
  expect(f.native.commit).not.toHaveBeenCalled();
  f.button(0)?.click();
  await drain();
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.native.commit).not.toHaveBeenCalled();
  f.form.close();
});
it("confirm is single use and only verified save refreshes the directory", async () => {
  const f = fixture();
  await f.form.prepare("sh", "delete");
  const old = f.button(1);
  old?.click();
  old?.click();
  await drain();
  expect(f.native.commit).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  expect(f.changed).toHaveBeenCalledOnce();
  expect(f.status()).toContain("Profile 已删除");
  old?.click();
  expect(f.native.commit).toHaveBeenCalledOnce();
  f.form.close();
});
it("read tombstone list is passive; restoration requires separate confirmation", async () => {
  const f = fixture();
  f.native.deleted.mockResolvedValue(["sh"]);
  await f.form.open();
  expect(f.native.prepare).not.toHaveBeenCalled();
  f.native.prepare.mockResolvedValue({ ...offer, action: "restore" });
  f.button(0)?.click();
  await drain();
  expect(f.status()).toContain("不会启动终端");
  expect(f.native.commit).not.toHaveBeenCalled();
  f.native.deleted.mockResolvedValue([]);
  f.button(1)?.click();
  await drain();
  expect(f.status()).toContain("Profile 已恢复");
  f.form.close();
});
it.each(["close", "page", "timeout"])(
  "late prepare after %s never redraws a new lifetime",
  async (mode) => {
    vi.useFakeTimers();
    const f = fixture();
    let resolve!: (v: unknown) => void;
    f.native.prepare.mockImplementation(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    const preparing = f.form.prepare("sh", "delete");
    if (mode === "close") f.form.close();
    else if (mode === "page") {
      f.page("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
      f.life.abort();
    } else await vi.advanceTimersByTimeAsync(5000);
    await preparing;
    const state = f.root.children;
    resolve(offer);
    await drain();
    expect(f.root.children).toBe(state);
    expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
    f.form.close();
  },
);
it("expired/old confirmation controls cannot issue a commit", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.form.prepare("sh", "delete");
  const old = f.button(1);
  await vi.advanceTimersByTimeAsync(55000);
  old?.click();
  expect(f.native.commit).not.toHaveBeenCalled();
  expect(f.status()).toContain("过期");
  f.form.close();
});
it("uncertain write shows no optimistic deletion or automatic retry", async () => {
  const f = fixture();
  f.native.commit.mockRejectedValue(Error("conflict"));
  await f.form.prepare("sh", "delete");
  f.button(1)?.click();
  await drain();
  expect(f.changed).not.toHaveBeenCalled();
  expect(f.status()).toContain("不自动重试");
  expect(f.native.commit).toHaveBeenCalledOnce();
  f.form.close();
});
it("close during commit cancels old authority without refreshing", async () => {
  const f = fixture();
  let resolve!: (v: unknown) => void;
  f.native.commit.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await f.form.prepare("sh", "delete");
  f.button(1)?.click();
  f.form.close();
  resolve(null);
  await drain();
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.changed).not.toHaveBeenCalled();
  expect(f.root.children).toHaveLength(0);
});
it("late list/refresh cannot clear new confirmation or its engaged state", async () => {
  const f = fixture();
  let resolve!: (v: unknown) => void;
  f.native.deleted.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const reading = f.form.open();
  await f.form.prepare("sh", "delete");
  const heading = f.root.children[0];
  resolve(["bash"]);
  await reading;
  expect(f.root.children[0]).toBe(heading);
  let refreshed!: () => void;
  f.changed.mockImplementationOnce(
    () =>
      new Promise<void>((r) => {
        refreshed = r;
      }),
  );
  f.button(1)?.click();
  await drain();
  f.form.close();
  await f.form.prepare("sh", "delete");
  f.engaged.mockClear();
  refreshed();
  await drain();
  expect(f.engaged).not.toHaveBeenCalled();
  expect(f.root.children[0]?.textContent).toBe("删除 POSIX sh");
  f.form.close();
});

it("silent nonce replacement revokes old prepared ticket without publishing or changing new state", async () => {
  const f = fixture();
  let resolve!: (v: unknown) => void;
  f.native.prepare.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const preparing = f.form.prepare("sh", "delete");
  f.page("cccccccc-cccc-4ccc-8ccc-cccccccccccc");
  f.engaged.mockClear();
  const original = f.root.children;
  resolve(offer);
  await preparing;
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.root.children).toBe(original);
  expect(f.engaged).not.toHaveBeenCalled();
  expect(f.native.commit).not.toHaveBeenCalled();
  f.form.close();
});
