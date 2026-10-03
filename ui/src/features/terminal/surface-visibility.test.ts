import { afterEach, expect, it, vi } from "vitest";
import {
  bindSurfaceVisibility,
  SurfaceVisibility,
  type VisibilityActions,
} from "./surface-visibility";

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});
function deferred() {
  let resolve!: () => void, reject!: (e: Error) => void;
  const promise = new Promise<void>((ok, no) => {
    resolve = ok;
    reject = no;
  });
  return { promise, resolve, reject };
}
function setup(overrides: Partial<VisibilityActions> = {}) {
  const abort = new AbortController();
  const actions = {
    retire: vi.fn(),
    pause: vi.fn().mockResolvedValue(undefined),
    resume: vi.fn().mockResolvedValue(undefined),
    failed: vi.fn(),
    ...overrides,
  };
  return { abort, actions, view: new SurfaceVisibility(actions, abort.signal) };
}
it("retires synchronously before dispatching pause, never merely on visible notifications", async () => {
  const s = setup();
  s.view.observe("document", true);
  expect(s.actions.retire).not.toHaveBeenCalled();
  s.view.observe("container", false);
  expect(s.actions.retire).toHaveBeenCalledTimes(1);
  expect(s.actions.pause).not.toHaveBeenCalled();
  await s.view.paused;
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
  expect(s.actions.resume).not.toHaveBeenCalled();
  s.view.dispose();
});
it("combines document and container hidden signals without a premature resume", async () => {
  const s = setup();
  s.view.observe("container", false);
  s.view.observe("document", false);
  await s.view.paused;
  s.view.observe("container", true);
  await Promise.resolve();
  expect(s.actions.resume).not.toHaveBeenCalled();
  s.view.observe("document", true);
  await s.view.resumed;
  expect(s.actions.resume).toHaveBeenCalledTimes(1);
});
it("atomically switches hidden sources without a transient visible resume", async () => {
  const s = setup();
  s.view.observeSnapshot({ container: false, document: true });
  await s.view.paused;
  s.view.observeSnapshot({ container: true, document: false });
  await Promise.resolve();
  expect(s.actions.resume).not.toHaveBeenCalled();
  expect(s.actions.failed).not.toHaveBeenCalled();
  s.view.observeSnapshot({ container: true, document: true });
  await s.view.resumed;
  expect(s.actions.resume).toHaveBeenCalledTimes(1);
});
it("resume timeout is uncertain and late failure cannot issue a second detach", async () => {
  vi.useFakeTimers();
  const resume = deferred(),
    s = setup({ resume: vi.fn(() => resume.promise) });
  s.view.observe("container", false);
  await s.view.paused;
  s.view.observe("container", true);
  await vi.advanceTimersByTimeAsync(1500);
  await expect(s.view.resumed).rejects.toThrow("uncertain");
  resume.reject(new Error("late"));
  await vi.advanceTimersByTimeAsync(5000);
  expect(s.actions.failed).toHaveBeenCalledTimes(1);
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
});
it("dispose during resume suppresses late success from an obsolete controller", async () => {
  const resume = deferred(),
    s = setup({ resume: vi.fn(() => resume.promise) });
  s.view.observe("container", false);
  await s.view.paused;
  s.view.observe("container", true);
  await Promise.resolve();
  s.view.dispose();
  resume.resolve();
  await expect(s.view.resumed).rejects.toThrow("cancelled");
  await Promise.resolve();
  expect(s.actions.failed).not.toHaveBeenCalled();
});

it("early show must wait for the pause receipt instead of skipping detach", async () => {
  const pause = deferred(),
    s = setup({ pause: vi.fn(() => pause.promise) });
  s.view.observe("container", false);
  s.view.observe("container", true);
  await Promise.resolve();
  expect(s.actions.resume).not.toHaveBeenCalled();
  pause.resolve();
  await s.view.resumed;
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
  expect(s.actions.resume).toHaveBeenCalledTimes(1);
});
it("coalesces repeated source storms and hands subsequent lifecycle to the new generation", async () => {
  const s = setup();
  for (let i = 0; i < 20; i++) {
    s.view.observe("container", false);
    s.view.observe("document", false);
  }
  await s.view.paused;
  s.view.observe("container", true);
  s.view.observe("document", true);
  await s.view.resumed;
  for (let i = 0; i < 10; i++) s.view.observe("container", false);
  expect(s.actions.retire).toHaveBeenCalledTimes(1);
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
  expect(s.actions.resume).toHaveBeenCalledTimes(1);
});
it("cancellation before invocation dispatches no stale control", async () => {
  const s = setup();
  s.view.observe("container", false);
  s.abort.abort();
  await expect(s.view.paused).rejects.toThrow("cancelled");
  await Promise.resolve();
  expect(s.actions.pause).not.toHaveBeenCalled();
  expect(s.actions.failed).not.toHaveBeenCalled();
});
it("cancellation during pause ignores late success and never resumes", async () => {
  const pause = deferred(),
    s = setup({ pause: vi.fn(() => pause.promise) });
  s.view.observe("container", false);
  await Promise.resolve();
  s.abort.abort();
  s.view.observe("container", true);
  pause.resolve();
  await expect(s.view.resumed).rejects.toThrow("cancelled");
  await Promise.resolve();
  expect(s.actions.resume).not.toHaveBeenCalled();
});
it("late failure from a disposed old generation cannot invalidate its replacement", async () => {
  const pause = deferred(),
    s = setup({ pause: vi.fn(() => pause.promise) });
  s.view.observe("container", false);
  await Promise.resolve();
  s.view.dispose();
  pause.reject(new Error("old"));
  await expect(s.view.paused).rejects.toThrow("cancelled");
  await Promise.resolve();
  expect(s.actions.failed).not.toHaveBeenCalled();
});
it("uncertain timeout rejects once and cannot retry after late success", async () => {
  vi.useFakeTimers();
  const pause = deferred(),
    s = setup({ pause: vi.fn(() => pause.promise) });
  s.view.observe("container", false);
  await vi.advanceTimersByTimeAsync(1500);
  await expect(s.view.paused).rejects.toThrow("uncertain");
  expect(s.actions.failed).toHaveBeenCalledTimes(1);
  s.view.observe("container", true);
  pause.resolve();
  await vi.advanceTimersByTimeAsync(5000);
  expect(s.actions.resume).not.toHaveBeenCalled();
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
});
it("pause errors fail closed without attempting resume", async () => {
  const s = setup({ pause: vi.fn().mockRejectedValue(new Error("denied")) });
  s.view.observe("container", false);
  s.view.observe("container", true);
  await expect(s.view.paused).rejects.toThrow("control_failed");
  expect(s.actions.failed).toHaveBeenCalledTimes(1);
  expect(s.actions.resume).not.toHaveBeenCalled();
});
it("resume errors fail closed without retrying the uncertain effect", async () => {
  const s = setup({ resume: vi.fn().mockRejectedValue(new Error("native")) });
  s.view.observe("container", false);
  await s.view.paused;
  s.view.observe("container", true);
  await expect(s.view.resumed).rejects.toThrow("control_failed");
  expect(s.actions.failed).toHaveBeenCalledTimes(1);
  s.view.observe("container", true);
  expect(s.actions.resume).toHaveBeenCalledTimes(1);
});
it("hidden during in-flight resume fails instead of issuing old-generation pause", async () => {
  const resume = deferred(),
    s = setup({ resume: vi.fn(() => resume.promise) });
  s.view.observe("container", false);
  await s.view.paused;
  s.view.observe("container", true);
  await Promise.resolve();
  s.view.observe("document", false);
  await expect(s.view.resumed).rejects.toThrow("changed_during_resume");
  resume.resolve();
  await Promise.resolve();
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
  expect(s.actions.failed).toHaveBeenCalledTimes(1);
});
it("retirement failure must not dispatch backend pause", async () => {
  const s = setup({
    retire: () => {
      throw new Error("renderer");
    },
  });
  s.view.observe("container", false);
  await expect(s.view.paused).rejects.toThrow("retire_failed");
  expect(s.actions.pause).not.toHaveBeenCalled();
});
it("already aborted view rejects waits and ignores all later signals", async () => {
  const abort = new AbortController();
  abort.abort();
  const actions = {
      retire: vi.fn(),
      pause: vi.fn(),
      resume: vi.fn(),
      failed: vi.fn(),
    },
    view = new SurfaceVisibility(actions, abort.signal);
  view.observe("document", false);
  await expect(view.paused).rejects.toThrow("cancelled");
  expect(actions.retire).not.toHaveBeenCalled();
});
it("binds real visibilitychange and hidden attributes, disconnecting listeners on abort", async () => {
  let mutation!: () => void;
  const disconnect = vi.fn();
  vi.stubGlobal(
    "MutationObserver",
    class {
      constructor(callback: () => void) {
        mutation = callback;
      }
      observe() {}
      disconnect() {
        disconnect();
      }
    },
  );
  const document = Object.assign(new EventTarget(), {
    visibilityState: "visible",
  });
  const host = { ownerDocument: document, isConnected: true, hidden: false };
  const s = setup();
  bindSurfaceVisibility(host as unknown as HTMLElement, s.view, s.abort.signal);
  host.hidden = true;
  mutation();
  await s.view.paused;
  document.visibilityState = "hidden";
  document.dispatchEvent(new Event("visibilitychange"));
  host.hidden = false;
  mutation();
  await Promise.resolve();
  expect(s.actions.resume).not.toHaveBeenCalled();
  document.visibilityState = "visible";
  document.dispatchEvent(new Event("visibilitychange"));
  await s.view.resumed;
  s.abort.abort();
  expect(disconnect).toHaveBeenCalledTimes(1);
  host.hidden = true;
  document.dispatchEvent(new Event("visibilitychange"));
  expect(s.actions.pause).toHaveBeenCalledTimes(1);
});
