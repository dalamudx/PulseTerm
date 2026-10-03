import { expect, it, vi } from "vitest";
import { GenerationViewHost, type ViewResource } from "./view-host";

function deferred<T>() {
  let resolve!: (v: T) => void, reject!: (e: Error) => void;
  const promise = new Promise<T>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return { promise, resolve, reject };
}
function setup() {
  const abort = new AbortController(),
    failed = vi.fn();
  return { abort, failed, host: new GenerationViewHost(abort.signal, failed) };
}
it("retirement settles a pending factory immediately and still disposes its late resource", async () => {
  const s = setup(),
    scope = s.host.begin(1n),
    ready = deferred<ViewResource>(),
    resource = { dispose: vi.fn() };
  const job = scope.create(() => ready.promise);
  await Promise.resolve();
  s.host.retire(1n);
  await expect(job).rejects.toThrow("retired");
  ready.resolve(resource);
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(resource.dispose).toHaveBeenCalledTimes(1);
});
it("retiring before async factory dispatch does not invoke a stale factory", async () => {
  const s = setup(),
    scope = s.host.begin(1n),
    factory = vi.fn(async () => ({ dispose: vi.fn() }));
  const job = scope.create(factory);
  s.host.retire(1n);
  await expect(job).rejects.toThrow("retired");
  expect(factory).not.toHaveBeenCalled();
});

it("late failing cleanup cannot close a healthy replacement", () => {
  const s = setup(),
    old = s.host.begin(1n);
  const next = s.host.begin(2n);
  expect(() =>
    old.own({
      dispose: () => {
        throw new Error("late");
      },
    }),
  ).toThrow("late_cleanup_failed");
  expect(next.isCurrent()).toBe(true);
  expect(s.failed).not.toHaveBeenCalled();
  s.host.dispose();
});
it("a resource re-registered after retirement is not disposed twice", () => {
  const s = setup(),
    old = s.host.begin(1n),
    resource = { dispose: vi.fn() };
  old.own(resource);
  s.host.retire(1n);
  expect(() => old.own(resource)).toThrow("retired");
  expect(resource.dispose).toHaveBeenCalledTimes(1);
});
it("new begin during an abort callback cannot overwrite and leak another scope", () => {
  const s = setup(),
    old = s.host.begin(1n);
  old.signal.addEventListener("abort", () => {
    expect(() => s.host.begin(3n)).toThrow("transition");
  });
  const next = s.host.begin(2n);
  expect(next.generation).toBe(2n);
  expect(next.isCurrent()).toBe(true);
  s.host.dispose();
});

it("one current scope owns each generation and revokes the old before creating a replacement", () => {
  const s = setup(),
    old = s.host.begin(1n),
    dispose = vi.fn();
  old.own({ dispose });
  const next = s.host.begin(2n);
  expect(old.signal.aborted).toBe(true);
  expect(old.isCurrent()).toBe(false);
  expect(dispose).toHaveBeenCalledTimes(1);
  expect(next.isCurrent()).toBe(true);
  expect(s.host.retire(1n)).toBe(false);
  expect(next.signal.aborted).toBe(false);
  s.host.dispose();
});
it("rejects zero, negative, replayed and out-of-u64 identities without revoking the good view", () => {
  const s = setup(),
    scope = s.host.begin(9007199254740993n);
  for (const g of [0n, -1n, 9007199254740993n, 9007199254740992n, 1n << 64n])
    expect(() => s.host.begin(g)).toThrow("generation");
  expect(scope.isCurrent()).toBe(true);
  s.host.dispose();
});
it("retired generations stay retired even while no new view is mounted", () => {
  const s = setup();
  s.host.begin(4n);
  expect(s.host.retire(4n)).toBe(true);
  expect(s.host.retire(4n)).toBe(false);
  expect(() => s.host.begin(4n)).toThrow("generation");
  expect(s.host.begin(5n).isCurrent()).toBe(true);
  s.host.dispose();
});
it("parent abort immediately releases current resources exactly once", () => {
  const s = setup(),
    scope = s.host.begin(1n),
    dispose = vi.fn();
  scope.own({ dispose });
  s.abort.abort();
  s.host.dispose();
  expect(scope.signal.aborted).toBe(true);
  expect(dispose).toHaveBeenCalledTimes(1);
  expect(() => s.host.begin(2n)).toThrow("closed");
});
it("already aborted host cannot invoke a view factory", () => {
  const a = new AbortController();
  a.abort();
  const host = new GenerationViewHost(a.signal, vi.fn());
  expect(() => host.begin(1n)).toThrow("closed");
});
it("deduplicates cleanup ownership and runs reverse-order cleanup", () => {
  const s = setup(),
    scope = s.host.begin(1n),
    order: string[] = [];
  const one = { dispose: () => order.push("one") },
    two = () => order.push("two");
  scope.own(one);
  scope.own(one);
  scope.own(two);
  scope.own(two);
  s.host.retire(1n);
  expect(order).toEqual(["two", "one"]);
});
it("late factory success is disposed without touching the new view", async () => {
  const s = setup(),
    old = s.host.begin(1n),
    ready = deferred<ViewResource>(),
    oldResource = { dispose: vi.fn() };
  const created = old.create(() => ready.promise);
  await Promise.resolve();
  const next = s.host.begin(2n),
    nextResource = { dispose: vi.fn() };
  next.own(nextResource);
  ready.resolve(oldResource);
  await expect(created).rejects.toThrow("retired");
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(oldResource.dispose).toHaveBeenCalledTimes(1);
  expect(nextResource.dispose).not.toHaveBeenCalled();
  expect(next.isCurrent()).toBe(true);
  s.host.dispose();
});
it("late factory rejection cannot retire the replacement or report its failure", async () => {
  const s = setup(),
    old = s.host.begin(1n),
    ready = deferred<ViewResource>();
  const created = old.create(() => ready.promise);
  await Promise.resolve();
  const next = s.host.begin(2n);
  ready.reject(new Error("old"));
  await expect(created).rejects.toThrow("retired");
  await Promise.resolve();
  await Promise.resolve();
  expect(next.isCurrent()).toBe(true);
  expect(s.failed).not.toHaveBeenCalled();
  s.host.dispose();
});
it("factory failure cleans already-owned resources and permits only a fresh generation", async () => {
  const s = setup(),
    scope = s.host.begin(1n),
    dispose = vi.fn();
  scope.own({ dispose });
  await expect(
    scope.create(async () => {
      throw new Error("partial");
    }),
  ).rejects.toThrow("creation_failed");
  expect(scope.signal.aborted).toBe(true);
  expect(dispose).toHaveBeenCalledTimes(1);
  expect(() => s.host.begin(1n)).toThrow("generation");
  expect(s.host.begin(2n).isCurrent()).toBe(true);
  s.host.dispose();
});
it("stale cleanup registration runs immediately instead of attaching to the new scope", () => {
  const s = setup(),
    old = s.host.begin(1n);
  const next = s.host.begin(2n),
    dispose = vi.fn();
  expect(() => old.own({ dispose })).toThrow("retired");
  expect(dispose).toHaveBeenCalledTimes(1);
  expect(next.isCurrent()).toBe(true);
  s.host.dispose();
});
it("one failing cleanup does not skip others and closes the host fail-safe", () => {
  const s = setup(),
    scope = s.host.begin(1n),
    clean = vi.fn();
  scope.own({ dispose: clean });
  scope.own({
    dispose: () => {
      throw new Error("bad");
    },
  });
  s.host.retire(1n);
  expect(clean).toHaveBeenCalledTimes(1);
  expect(s.failed).toHaveBeenCalledTimes(1);
  expect(() => s.host.begin(2n)).toThrow("closed");
});
it("cancellation while factory is pending releases its eventual result", async () => {
  const s = setup(),
    scope = s.host.begin(1n),
    ready = deferred<ViewResource>(),
    resource = { dispose: vi.fn() };
  const job = scope.create(() => ready.promise);
  await Promise.resolve();
  s.abort.abort();
  ready.resolve(resource);
  await expect(job).rejects.toThrow("retired");
  await Promise.resolve();
  await Promise.resolve();
  await Promise.resolve();
  expect(resource.dispose).toHaveBeenCalledTimes(1);
});
it("reentrant abort callbacks cannot revoke a future scope", () => {
  const s = setup(),
    old = s.host.begin(1n);
  old.signal.addEventListener("abort", () => s.host.retire(1n));
  const next = s.host.begin(2n);
  expect(next.isCurrent()).toBe(true);
  s.host.dispose();
});
it("factory receives its own lifetime and returned resources are owned automatically", async () => {
  const s = setup(),
    scope = s.host.begin(1n),
    resource = { dispose: vi.fn() };
  const factory = vi.fn(async (signal: AbortSignal) => {
    expect(signal).toBe(scope.signal);
    return resource;
  });
  expect(await scope.create(factory)).toBe(resource);
  s.host.dispose();
  expect(resource.dispose).toHaveBeenCalledTimes(1);
});
