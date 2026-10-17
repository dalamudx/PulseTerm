import { afterEach, expect, it, vi } from "vitest";
import type { NativeVisibilitySample } from "../../ipc/gen/commands";
import {
  observeWorkspaceVisibility,
  prepareWorkspaceVisibility,
  type VisibilityDependencies,
  WorkspaceVisibility,
} from "./workspace-visibility";

const host = vi.hoisted(() => ({ listen: vi.fn(), invoke: vi.fn() }));
vi.mock("@tauri-apps/api/core", () => ({ invoke: host.invoke }));
vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({ listen: host.listen }),
}));
const sample: NativeVisibilitySample = {
  incarnation: "window-a",
  revision: 1,
  visible: true,
  minimized: false,
  valid: true,
};
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function fixture() {
  let receive!: (sample: NativeVisibilitySample) => void;
  const document = Object.assign(new EventTarget(), {
    visibilityState: "visible" as DocumentVisibilityState,
  });
  const snapshot = deferred<NativeVisibilitySample>(),
    stop = vi.fn(),
    changed = vi.fn(),
    failed = vi.fn();
  const deps: VisibilityDependencies = {
    listen: vi.fn(async (next) => {
      receive = next;
      return stop;
    }),
    snapshot: vi.fn(() => snapshot.promise),
  };
  const source = new WorkspaceVisibility(document, "original-page", deps);
  const unsubscribe = source.subscribe(changed, failed);
  return {
    document,
    snapshot,
    stop,
    changed,
    failed,
    deps,
    source,
    unsubscribe,
    receive: (value: NativeVisibilitySample) => receive(value),
  };
}
afterEach(() => vi.useRealTimers());
it("native minimization pauses a DOM-visible view, focus-like visible samples do not", async () => {
  const f = fixture();
  expect(f.changed).toHaveBeenLastCalledWith(false);
  f.snapshot.resolve(sample);
  await f.source.ready;
  f.receive({ ...sample, revision: 2, minimized: true });
  expect(f.changed).toHaveBeenLastCalledWith(false);
  f.receive({ ...sample, revision: 3 });
  expect(f.changed).toHaveBeenLastCalledWith(true);
  f.unsubscribe();
  expect(f.stop).toHaveBeenCalledOnce();
});
it("native and document visibility must both recover before resuming", async () => {
  const f = fixture();
  f.snapshot.resolve(sample);
  await f.source.ready;
  f.document.visibilityState = "hidden";
  f.document.dispatchEvent(new Event("visibilitychange"));
  f.receive({ ...sample, revision: 2, visible: false });
  f.receive({ ...sample, revision: 3 });
  expect(f.changed).toHaveBeenLastCalledWith(false);
  f.document.visibilityState = "visible";
  f.document.dispatchEvent(new Event("visibilitychange"));
  expect(f.changed).toHaveBeenLastCalledWith(true);
  f.unsubscribe();
});
it("listens before snapshot and keeps a newer matching event while ignoring foreign/stale events", async () => {
  const f = fixture();
  await Promise.resolve();
  f.receive({ ...sample, revision: 5, minimized: true });
  f.receive({ ...sample, incarnation: "foreign", revision: 90 });
  f.snapshot.resolve({ ...sample, revision: 3 });
  await f.source.ready;
  expect(f.changed).toHaveBeenLastCalledWith(false);
  f.receive({ ...sample, revision: 4 });
  f.receive({ ...sample, incarnation: "foreign", revision: 100 });
  expect(f.changed).toHaveBeenLastCalledWith(false);
  f.unsubscribe();
});
it("shares a source among panes and unlistens only after the last pane", async () => {
  const f = fixture(),
    other = vi.fn();
  const stop = f.source.subscribe(other, vi.fn());
  f.snapshot.resolve(sample);
  await f.source.ready;
  f.unsubscribe();
  expect(f.stop).not.toHaveBeenCalled();
  f.receive({ ...sample, revision: 2, minimized: true });
  expect(other).toHaveBeenLastCalledWith(false);
  stop();
  expect(f.stop).toHaveBeenCalledOnce();
  expect(f.deps.listen).toHaveBeenCalledOnce();
  expect(f.deps.snapshot).toHaveBeenCalledWith("original-page");
});
it("a failed current projection stops views and cannot be reopened by later samples", async () => {
  const f = fixture();
  f.snapshot.resolve(sample);
  await f.source.ready;
  f.receive({ ...sample, revision: 2, valid: false });
  f.receive({ ...sample, revision: 3 });
  expect(f.changed).toHaveBeenLastCalledWith(false);
  expect(f.failed).toHaveBeenCalledOnce();
  expect(f.stop).toHaveBeenCalledOnce();
  f.unsubscribe();
});
it("initial snapshot failure closes startup rather than defaulting visible", async () => {
  const f = fixture();
  f.snapshot.resolve({ ...sample, valid: false });
  await expect(f.source.ready).rejects.toThrow("visibility_failed");
  expect(f.changed).toHaveBeenLastCalledWith(false);
  expect(f.failed).toHaveBeenCalledOnce();
  f.unsubscribe();
});
it("late snapshot cannot publish after every subscriber exits", async () => {
  const f = fixture();
  await Promise.resolve();
  f.unsubscribe();
  f.snapshot.resolve(sample);
  await expect(f.source.ready).rejects.toThrow("retired");
  await Promise.resolve();
  expect(f.changed).not.toHaveBeenCalledWith(true);
  expect(f.stop).toHaveBeenCalledOnce();
});
it("late listener registration is removed after bounded startup timeout", async () => {
  vi.useFakeTimers();
  const listening = deferred<() => void>(),
    stop = vi.fn(),
    failed = vi.fn();
  const source = new WorkspaceVisibility(
    Object.assign(new EventTarget(), {
      visibilityState: "visible" as DocumentVisibilityState,
    }),
    "page",
    { listen: () => listening.promise, snapshot: vi.fn() },
  );
  const unsubscribe = source.subscribe(vi.fn(), failed);
  await vi.advanceTimersByTimeAsync(2000);
  await expect(source.ready).rejects.toThrow("visibility_timeout");
  listening.resolve(stop);
  await Promise.resolve();
  expect(stop).toHaveBeenCalledOnce();
  expect(failed).toHaveBeenCalledOnce();
  unsubscribe();
});

it("page startup awaits the native snapshot and retains one shared listener until page exit", async () => {
  const document = Object.assign(new EventTarget(), {
    visibilityState: "visible" as DocumentVisibilityState,
  }) as unknown as Document;
  const snapshot = deferred<NativeVisibilitySample>(),
    stop = vi.fn(),
    life = new AbortController();
  host.listen.mockReset().mockResolvedValue(stop);
  host.invoke.mockReset().mockReturnValue(snapshot.promise);
  let ready = false;
  const boot = prepareWorkspaceVisibility(document, "page", life.signal).then(
    () => {
      ready = true;
    },
  );
  const changed = vi.fn(),
    leave = observeWorkspaceVisibility(document, "page", changed, vi.fn());
  await Promise.resolve();
  expect(ready).toBe(false);
  expect(host.listen).toHaveBeenCalledOnce();
  snapshot.resolve(sample);
  await boot;
  expect(changed).toHaveBeenLastCalledWith(true);
  leave();
  expect(stop).not.toHaveBeenCalled();
  life.abort();
  expect(stop).toHaveBeenCalledOnce();
});
it("page exit while listener setup is pending cannot read or publish a stale snapshot", async () => {
  const document = Object.assign(new EventTarget(), {
    visibilityState: "visible" as DocumentVisibilityState,
  }) as unknown as Document;
  const listening = deferred<() => void>(),
    stop = vi.fn(),
    life = new AbortController();
  host.listen.mockReset().mockReturnValue(listening.promise);
  host.invoke.mockReset();
  const boot = prepareWorkspaceVisibility(document, "page", life.signal);
  life.abort();
  await expect(boot).rejects.toThrow("retired");
  listening.resolve(stop);
  await vi.waitFor(() => expect(stop).toHaveBeenCalledOnce());
  expect(host.invoke).not.toHaveBeenCalled();
});
