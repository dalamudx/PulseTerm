import { afterEach, expect, it, vi } from "vitest";
import { DefaultProfileForm } from "./workspace-profile-default-form";

class Node {
  children: Node[] = [];
  value = "";
  textContent = "";
  disabled = false;
  type = "";
  className = "";
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
  emit(event: string) {
    for (const fn of this.listeners.get(event) ?? []) fn();
  }
}
const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const offer = {
  ticket,
  selected: "sh",
  available: true,
  offered: ["sh", "bash"],
};
function fixture() {
  const root = new Node(),
    life = new AbortController();
  let page = nonce;
  const native = {
    read: vi.fn(async (): Promise<unknown> => offer),
    save: vi.fn(
      async (
        _nonce: string,
        _ticket: string,
        _selected: unknown,
      ): Promise<unknown> => null,
    ),
    cancel: vi.fn(async () => null),
  };
  const launch = vi.fn(),
    working = vi.fn();
  const form = new DefaultProfileForm({
    root: root as unknown as HTMLElement,
    signal: life.signal,
    nonce: () => page,
    native,
    launch,
    working,
  });
  return {
    root,
    life,
    native,
    launch,
    working,
    form,
    page: (v: string) => {
      page = v;
    },
    select: () => root.children[0]?.children[1],
    button: (n: number) => root.children[1]?.children[n],
    status: () => root.children[2]?.textContent,
  };
}
async function drain() {
  for (let i = 0; i < 15; i++) await Promise.resolve();
}
afterEach(() => vi.useRealTimers());
it("shows persisted choice; uncommitted changes disable start and save never launches", async () => {
  const f = fixture();
  await f.form.open();
  expect(f.select()?.value).toBe("sh");
  expect(f.button(0)?.disabled).toBe(true);
  expect(f.button(1)?.disabled).toBe(false);
  const old = f.button(0);
  const select = f.select();
  if (select) {
    select.value = "bash";
    select.emit("change");
  }
  expect(f.button(1)?.disabled).toBe(true);
  f.button(1)?.emit("click");
  expect(f.launch).not.toHaveBeenCalled();
  f.native.read.mockResolvedValue({ ...offer, selected: "bash" });
  f.button(0)?.emit("click");
  await drain();
  expect(f.native.save).toHaveBeenCalledExactlyOnceWith(nonce, ticket, "bash");
  expect(f.select()?.value).toBe("bash");
  expect(f.status()).toBe("默认选择已保存");
  expect(f.launch).not.toHaveBeenCalled();
  old?.emit("click");
  expect(f.native.save).toHaveBeenCalledTimes(1);
  f.form.close();
});
it("default start rechecks same saved identity then launches once with original nonce", async () => {
  const f = fixture();
  await f.form.open();
  const start = f.button(1);
  start?.emit("click");
  start?.emit("click");
  await drain();
  expect(f.native.read).toHaveBeenCalledTimes(2);
  expect(f.launch).toHaveBeenCalledExactlyOnceWith(nonce, "sh");
  expect(f.native.save).not.toHaveBeenCalled();
  expect(f.root.children).toHaveLength(0);
  f.form.close();
});
it("external default replacement stops startup and shows replacement for a new click", async () => {
  const f = fixture();
  await f.form.open();
  f.native.read.mockResolvedValue({ ...offer, selected: "bash" });
  f.button(1)?.emit("click");
  await drain();
  expect(f.launch).not.toHaveBeenCalled();
  expect(f.select()?.value).toBe("bash");
  expect(f.status()).toContain("已变化");
  f.form.close();
});
it("unavailable saved identity is visible and cannot start or be silently replaced", async () => {
  const f = fixture();
  f.native.read.mockResolvedValue({
    ...offer,
    selected: "fish",
    available: false,
  });
  await f.form.open();
  expect(f.select()?.value).toBe("fish");
  expect(f.status()).toContain("不可用");
  expect(f.button(1)?.disabled).toBe(true);
  f.button(1)?.emit("click");
  expect(f.launch).not.toHaveBeenCalled();
  const select = f.select();
  if (select) {
    select.value = "";
    select.emit("change");
  }
  f.native.read.mockResolvedValue({
    ...offer,
    selected: null,
    available: false,
  });
  f.button(0)?.emit("click");
  await drain();
  expect(f.native.save).toHaveBeenCalledExactlyOnceWith(nonce, ticket, null);
  expect(f.select()?.value).toBe("");
  f.form.close();
});
it("close during startup recheck retires receipt and old DOM", async () => {
  const f = fixture();
  await f.form.open();
  let resolve!: (v: unknown) => void;
  f.native.read.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const old = f.button(1);
  old?.emit("click");
  f.form.close();
  resolve(offer);
  await drain();
  old?.emit("click");
  expect(f.launch).not.toHaveBeenCalled();
  expect(f.root.children).toHaveLength(0);
  expect(f.native.cancel).toHaveBeenCalled();
});
it("page abort during a write cannot reopen or publish saved state", async () => {
  const f = fixture();
  await f.form.open();
  let resolve!: (v: unknown) => void;
  f.native.save.mockImplementation(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const select = f.select();
  if (select) {
    select.value = "bash";
    select.emit("change");
  }
  f.button(0)?.emit("click");
  f.life.abort();
  resolve(null);
  await drain();
  expect(f.root.children).toHaveLength(0);
  expect(f.native.read).toHaveBeenCalledTimes(1);
  expect(f.launch).not.toHaveBeenCalled();
});
it("uncertain write requires explicit reread and has no optimistic replacement or retry", async () => {
  const f = fixture();
  await f.form.open();
  f.native.save.mockRejectedValue(Error("conflict"));
  const select = f.select();
  if (select) {
    select.value = "bash";
    select.emit("change");
  }
  f.button(0)?.emit("click");
  await drain();
  expect(f.status()).toContain("结果未确认");
  expect(f.button(0)?.disabled).toBe(true);
  expect(f.button(1)?.disabled).toBe(true);
  expect(f.native.save).toHaveBeenCalledTimes(1);
  expect(f.native.read).toHaveBeenCalledTimes(1);
  f.form.close();
});
it("expiry disables actions and old-render click cannot reread or start", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.form.open();
  const old = f.button(1);
  await vi.advanceTimersByTimeAsync(55000);
  expect(f.status()).toContain("过期");
  expect(f.button(1)?.disabled).toBe(true);
  old?.emit("click");
  expect(f.native.read).toHaveBeenCalledTimes(1);
  expect(f.launch).not.toHaveBeenCalled();
  f.form.close();
});
