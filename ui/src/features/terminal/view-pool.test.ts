import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { type PooledViewController, ViewHost } from "./view-pool";
import { viewSlotRef } from "./view-slot-ref";

class Node {
  hidden = false;
  inert = false;
  parentElement: Node | null = null;
  children: Node[] = [];
  get isConnected(): boolean {
    return (
      this === this.ownerDocument.root || !!this.parentElement?.isConnected
    );
  }
  constructor(readonly ownerDocument: Doc) {}
  appendChild(n: Node) {
    n.remove();
    this.children.push(n);
    n.parentElement = this;
    return n;
  }
  remove() {
    if (this.parentElement)
      this.parentElement.children = this.parentElement.children.filter(
        (n) => n !== this,
      );
    this.parentElement = null;
  }
  contains(n: Node): boolean {
    return this === n || this.children.some((c) => c.contains(n));
  }
}
class Doc {
  root = new Node(this);
  createElement() {
    return new Node(this);
  }
}
const asElement = (n: Node) => n as unknown as HTMLElement;
const all: ViewHost[] = [];
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  for (const p of all.splice(0)) p.dispose();
  vi.clearAllTimers();
  vi.useRealTimers();
});
function fixture(
  policy: { maxViews?: number; maxParked?: number; ttlMs?: number } = {},
) {
  const doc = new Doc(),
    parking = doc.createElement(),
    one = doc.createElement(),
    two = doc.createElement();
  doc.root.appendChild(parking);
  doc.root.appendChild(one);
  doc.root.appendChild(two);
  let time = 0;
  const abort = new AbortController(),
    failed = vi.fn();
  const pool = new ViewHost(
    asElement(parking),
    abort.signal,
    failed,
    policy,
    () => time,
  );
  all.push(pool);
  return {
    doc,
    parking,
    one: asElement(one),
    two: asElement(two),
    pool,
    failed,
    abort,
    advance: (ms: number) => {
      time += ms;
      pool.sweep();
    },
  };
}
function controller() {
  return { setVisible: vi.fn(), dispose: vi.fn() };
}
function deferred() {
  let resolve!: () => void, reject!: (e: Error) => void;
  const promise = new Promise<void>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return { promise, resolve, reject };
}
it("two panes cannot occupy one slot or dispatch a second controller factory", () => {
  const f = fixture(),
    a = f.pool.mount("a", f.one, () => controller()),
    factory = vi.fn(() => controller());
  expect(() => f.pool.mount("b", f.one, factory)).toThrow("slot_occupied");
  expect(factory).not.toHaveBeenCalled();
  expect(a.isCurrent()).toBe(true);
});
it("invalid async visibility adapter holds capacity until its actual tail finishes", async () => {
  const f = fixture({ maxViews: 1, maxParked: 0 }),
    p = deferred();
  expect(() =>
    f.pool.mount("bad", f.one, () => ({
      setVisible: () => p.promise,
      dispose: () => {},
    })),
  ).toThrow("mount_failed");
  await vi.advanceTimersByTimeAsync(1500);
  expect(f.pool.pendingDisposals).toBe(1);
  expect(() => f.pool.mount("new", f.two, () => controller())).toThrow(
    "capacity",
  );
  p.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.pool.pendingDisposals).toBe(0);
});
it("overlapping visibility and disposal tails do not release the entry before both finish", async () => {
  const f = fixture({ maxViews: 1, maxParked: 0 }),
    visible = deferred(),
    disposed = deferred();
  expect(() =>
    f.pool.mount("bad", f.one, () => ({
      setVisible: () => visible.promise,
      dispose: () => disposed.promise,
    })),
  ).toThrow("mount_failed");
  visible.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.pool.pendingDisposals).toBe(1);
  disposed.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.pool.pendingDisposals).toBe(0);
});

it("React19 ref cleanup only parks its own ticket across stale callback and remount", () => {
  const f = fixture(),
    c = controller(),
    factory = vi.fn(() => c),
    attach = viewSlotRef(f.pool, "pane", factory);
  const cleanupA = attach(f.one as HTMLDivElement);
  const cleanupB = attach(f.two as HTMLDivElement);
  cleanupA?.();
  expect(c.setVisible.mock.calls).toEqual([[true]]);
  cleanupB?.();
  const cleanupC = attach(f.one as HTMLDivElement);
  expect(factory).toHaveBeenCalledTimes(1);
  expect(c.setVisible.mock.calls).toEqual([[true], [false], [true]]);
  cleanupC?.();
});
it("async visibility adapters are rejected and their late failure cannot be unhandled", async () => {
  const f = fixture(),
    p = deferred(),
    dispose = vi.fn();
  expect(() =>
    f.pool.mount("bad", f.one, () => ({
      setVisible: () => p.promise,
      dispose,
    })),
  ).toThrow("mount_failed");
  p.reject(new Error("late"));
  await Promise.resolve();
  await Promise.resolve();
  expect(dispose).toHaveBeenCalledTimes(1);
  expect(f.pool.size).toBe(0);
});

it("key reuse across slots preserves element/controller and rejects late old release", () => {
  const f = fixture(),
    c = controller(),
    factory = vi.fn(() => c);
  const old = f.pool.mount("pane", f.one, factory);
  const next = f.pool.mount("pane", f.two, factory);
  expect(next.element).toBe(old.element);
  expect(factory).toHaveBeenCalledTimes(1);
  expect(old.signal.aborted).toBe(true);
  expect(old.release()).toBe(false);
  expect(next.isCurrent()).toBe(true);
  expect(c.dispose).not.toHaveBeenCalled();
  expect(c.setVisible.mock.calls).toEqual([[true]]);
});
it("strict-style mount release remount keeps a single controller and element", () => {
  const f = fixture(),
    c = controller(),
    factory = vi.fn(() => c);
  const a = f.pool.mount("pane", f.one, factory);
  expect(a.release()).toBe(true);
  const b = f.pool.mount("pane", f.one, factory);
  expect(b.element).toBe(a.element);
  expect(factory).toHaveBeenCalledTimes(1);
  expect(c.setVisible.mock.calls).toEqual([[true], [false], [true]]);
  expect(c.dispose).not.toHaveBeenCalled();
});
it("parking is hidden inert and leaves controller lifetime alive", () => {
  const f = fixture(),
    c = controller();
  let life!: AbortSignal;
  const a = f.pool.mount("pane", f.one, (_el, s) => {
    life = s;
    return c;
  });
  a.release();
  expect(f.parking.hidden).toBe(true);
  expect(f.parking.inert).toBe(true);
  expect(a.element.parentElement).toBe(f.parking);
  expect(a.signal.aborted).toBe(true);
  expect(life.aborted).toBe(false);
  expect(c.dispose).not.toHaveBeenCalled();
});
it("LRU eviction affects oldest parked entry but never mounted entries", () => {
  const f = fixture({ maxViews: 3, maxParked: 1 }),
    c = controller(),
    d = controller(),
    active = controller();
  f.pool.mount("active", f.one, () => active);
  f.pool.mount("old", f.two, () => c).release();
  f.pool.mount("recent", f.two, () => d).release();
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(d.dispose).not.toHaveBeenCalled();
  expect(active.dispose).not.toHaveBeenCalled();
  expect(f.pool.size).toBe(2);
});
it("TTL removes parked but not active resources exactly once", () => {
  const f = fixture({ ttlMs: 100 }),
    c = controller(),
    d = controller();
  f.pool.mount("parked", f.one, () => c).release();
  f.pool.mount("active", f.two, () => d);
  f.advance(99);
  expect(c.dispose).not.toHaveBeenCalled();
  f.advance(1);
  expect(c.dispose).toHaveBeenCalledTimes(1);
  f.advance(500);
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(d.dispose).not.toHaveBeenCalled();
});
it("mounted capacity is bounded and rejects new factory without evicting active view", () => {
  const f = fixture({ maxViews: 1, maxParked: 1 }),
    c = controller(),
    factory = vi.fn(() => controller());
  f.pool.mount("one", f.one, () => c);
  expect(() => f.pool.mount("two", f.two, factory)).toThrow("capacity");
  expect(factory).not.toHaveBeenCalled();
  expect(c.dispose).not.toHaveBeenCalled();
});
it("factory failure aborts its lifetime and removes only its own container", () => {
  const f = fixture();
  let signal!: AbortSignal;
  expect(() =>
    f.pool.mount("bad", f.one, (_e, s) => {
      signal = s;
      throw new Error("bad");
    }),
  ).toThrow("creation_failed");
  expect(signal.aborted).toBe(true);
  expect(f.pool.size).toBe(0);
  expect(f.one.children.length).toBe(0);
  expect(f.pool.mount("good", f.two, () => controller()).isCurrent()).toBe(
    true,
  );
});
it("visibility callback failure removes the failed view and disposes its controller", () => {
  const f = fixture(),
    c = controller();
  c.setVisible.mockImplementation(() => {
    throw new Error("bad");
  });
  expect(() => f.pool.mount("bad", f.one, () => c)).toThrow("mount_failed");
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(f.pool.size).toBe(0);
});
it("park callback failure cannot preserve a falsely reusable view", () => {
  const f = fixture(),
    c = controller(),
    a = f.pool.mount("pane", f.one, () => c);
  c.setVisible.mockImplementation(() => {
    throw new Error("bad");
  });
  expect(() => a.release()).toThrow("park_failed");
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(a.isCurrent()).toBe(false);
});
it("parent abort cancels mounts and all controller lifetimes without duplicate disposal", () => {
  const f = fixture(),
    c = controller(),
    d = controller();
  const a = f.pool.mount("a", f.one, () => c);
  f.pool.mount("b", f.two, () => d).release();
  f.abort.abort();
  f.pool.dispose();
  expect(a.signal.aborted).toBe(true);
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(d.dispose).toHaveBeenCalledTimes(1);
  expect(a.release()).toBe(false);
  expect(() => f.pool.mount("new", f.one, () => controller())).toThrow(
    "closed",
  );
});
it("one throwing disposer does not skip other views", () => {
  const f = fixture(),
    c = controller(),
    d = controller();
  c.dispose.mockImplementation(() => {
    throw new Error("bad");
  });
  f.pool.mount("a", f.one, () => c);
  f.pool.mount("b", f.two, () => d);
  f.pool.dispose();
  expect(d.dispose).toHaveBeenCalledTimes(1);
  expect(f.failed).toHaveBeenCalledTimes(1);
});
it("pending disposal still consumes capacity after its timeout until actual completion", async () => {
  const f = fixture({ maxViews: 1, maxParked: 0 }),
    p = deferred();
  const c: PooledViewController = {
    setVisible: () => {},
    dispose: () => p.promise,
  };
  f.pool.mount("a", f.one, () => c).release();
  expect(f.pool.pendingDisposals).toBe(1);
  await vi.advanceTimersByTimeAsync(1500);
  expect(f.failed).toHaveBeenCalledWith(
    "a",
    expect.objectContaining({ message: "pool.dispose_uncertain" }),
  );
  expect(() => f.pool.mount("a", f.one, () => controller())).toThrow(
    "capacity",
  );
  p.resolve();
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(f.pool.pendingDisposals).toBe(0);
  expect(f.pool.mount("a", f.one, () => controller()).isCurrent()).toBe(true);
});
it("late old disposal failure cannot remove a newer view with the same key", async () => {
  const f = fixture({ maxViews: 2, maxParked: 0 }),
    p = deferred();
  f.pool
    .mount("pane", f.one, () => ({
      setVisible: () => {},
      dispose: () => p.promise,
    }))
    .release();
  const c = controller(),
    next = f.pool.mount("pane", f.two, () => c);
  p.reject(new Error("old"));
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(next.isCurrent()).toBe(true);
  expect(c.dispose).not.toHaveBeenCalled();
  expect(f.pool.size).toBe(1);
});
it("cyclic reparenting is rejected without revoking an existing mount", () => {
  const f = fixture(),
    a = f.pool.mount("pane", f.one, () => controller());
  expect(() => f.pool.mount("pane", a.element, () => controller())).toThrow(
    "cyclic_slot",
  );
  expect(a.isCurrent()).toBe(true);
});
it("cross-document disconnected or parked slots are rejected before creating resources", () => {
  const f = fixture(),
    factory = vi.fn(() => controller()),
    other = new Doc();
  for (const slot of [
    asElement(other.root),
    asElement(f.doc.createElement()),
    asElement(f.parking),
  ])
    expect(() => f.pool.mount("pane", slot, factory)).toThrow("slot");
  expect(factory).not.toHaveBeenCalled();
});
it("policy and key validation fail before changing a good mount", () => {
  const f = fixture(),
    a = f.pool.mount("pane", f.one, () => controller());
  for (const key of ["", "x".repeat(129)])
    expect(() => f.pool.mount(key, f.two, () => controller())).toThrow("slot");
  expect(a.isCurrent()).toBe(true);
  for (const policy of [
    { maxViews: 0 },
    { maxViews: 65 },
    { maxParked: -1 },
    { ttlMs: 0 },
    { ttlMs: Infinity },
  ])
    expect(
      () =>
        new ViewHost(
          asElement(f.parking),
          new AbortController().signal,
          vi.fn(),
          policy,
        ),
    ).toThrow("policy");
});
it("factory abort during creation still disposes its returned resource", () => {
  const f = fixture(),
    c = controller();
  expect(() =>
    f.pool.mount("pane", f.one, () => {
      f.abort.abort();
      return c;
    }),
  ).toThrow("creation_failed");
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(f.pool.size).toBe(0);
});
it("timer is one-shot and removed when the last parked view remounts", () => {
  const f = fixture();
  const a = f.pool.mount("pane", f.one, () => controller());
  a.release();
  expect(vi.getTimerCount()).toBe(1);
  f.pool.mount("pane", f.two, () => controller());
  expect(vi.getTimerCount()).toBe(0);
});
it("zero parking allowance evicts immediately without closing a session elsewhere", () => {
  const f = fixture({ maxParked: 0 }),
    c = controller();
  f.pool.mount("pane", f.one, () => c).release();
  expect(c.setVisible.mock.calls).toEqual([[true], [false]]);
  expect(c.dispose).toHaveBeenCalledTimes(1);
  expect(f.pool.size).toBe(0);
});
it("stale mount abort handlers cannot park the replacement", () => {
  const f = fixture(),
    a = f.pool.mount("pane", f.one, () => controller());
  a.signal.addEventListener("abort", () => {
    expect(a.release()).toBe(false);
  });
  const b = f.pool.mount("pane", f.two, () => controller());
  expect(b.isCurrent()).toBe(true);
});
