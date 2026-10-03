import { expect, it, vi } from "vitest";
import { PaneVisibility } from "./pane-visibility";

it("late already-aborted bind cannot clear a currently active controller", () => {
  const source = new PaneVisibility(),
    live = vi.fn(),
    old = vi.fn(),
    aborted = new AbortController();
  source.bind(live, new AbortController().signal);
  aborted.abort();
  source.bind(old, aborted.signal)();
  source.observe(false);
  expect(live.mock.calls).toEqual([[true], [false]]);
  expect(old).not.toHaveBeenCalled();
});
it("notification failure retires that control-plane binding rather than claiming future changes handled", () => {
  const source = new PaneVisibility();
  source.bind((v) => {
    if (!v) throw new Error("bad");
  }, new AbortController().signal);
  expect(() => source.observe(false)).toThrow("visibility_failed");
  expect(source.observe(true)).toBe(false);
});

it("pool hidden notification is delivered synchronously before returning", () => {
  const source = new PaneVisibility(),
    order: string[] = [];
  source.bind((v) => {
    if (!v) order.push("retired");
  }, new AbortController().signal);
  expect(source.observe(false)).toBe(true);
  order.push("returned");
  expect(order).toEqual(["retired", "returned"]);
});
it("new attachment gets the actual current hidden snapshot", () => {
  const source = new PaneVisibility();
  expect(source.observe(false)).toBe(false);
  const listener = vi.fn();
  source.bind(listener, new AbortController().signal);
  expect(listener).toHaveBeenCalledWith(false);
  expect(source.read()).toBe(false);
});
it("old subscription stop or abort cannot remove the replacement controller", () => {
  const source = new PaneVisibility(),
    oldAbort = new AbortController(),
    old = vi.fn(),
    next = vi.fn();
  const stop = source.bind(old, oldAbort.signal);
  source.bind(next, new AbortController().signal);
  stop();
  oldAbort.abort();
  source.observe(false);
  expect(next.mock.calls).toEqual([[true], [false]]);
  expect(old.mock.calls).toEqual([[true]]);
});
it("repeated visible/hidden samples do not repeat detach or resume", () => {
  const source = new PaneVisibility(),
    listener = vi.fn();
  source.bind(listener, new AbortController().signal);
  source.observe(false);
  source.observe(false);
  source.observe(true);
  source.observe(true);
  expect(listener.mock.calls).toEqual([[true], [false], [true]]);
});
it("aborted control plane unbinds so caller can fail closed instead of claiming handled", () => {
  const source = new PaneVisibility(),
    abort = new AbortController(),
    listener = vi.fn();
  source.bind(listener, abort.signal);
  abort.abort();
  expect(source.observe(false)).toBe(false);
  expect(listener).toHaveBeenCalledTimes(1);
});
it("already-aborted bindings are never activated or receive snapshots", () => {
  const source = new PaneVisibility(),
    abort = new AbortController(),
    listener = vi.fn();
  abort.abort();
  source.bind(listener, abort.signal);
  expect(source.observe(false)).toBe(false);
  expect(listener).not.toHaveBeenCalled();
});
it("snapshot listener failure cannot leave a partly initialized controller", () => {
  const source = new PaneVisibility();
  expect(() =>
    source.bind(() => {
      throw new Error("bad");
    }, new AbortController().signal),
  ).toThrow("visibility_failed");
  expect(source.observe(false)).toBe(false);
});
