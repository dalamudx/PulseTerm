import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { armViewDeadline } from "./view-deadline";
import { GenerationViewHost } from "./view-host";

beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});
it("current scope fails once at the finite deadline without silently extending it", () => {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn()),
    scope = host.begin(1n),
    failed = vi.fn();
  armViewDeadline(scope, 6000, "session.fit_no_transition_0", failed);
  vi.advanceTimersByTime(5999);
  expect(failed).not.toHaveBeenCalled();
  vi.advanceTimersByTime(1);
  expect(failed).toHaveBeenCalledWith(
    expect.objectContaining({ message: "session.fit_no_transition_0" }),
  );
  vi.advanceTimersByTime(10000);
  expect(failed).toHaveBeenCalledTimes(1);
  host.dispose();
});
it("old generation timers are cleared and cannot fail a new scope", () => {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn()),
    scope = host.begin(1n),
    failed = vi.fn();
  armViewDeadline(scope, 6000, "old.fit", failed);
  const next = host.begin(2n);
  vi.advanceTimersByTime(10000);
  expect(failed).not.toHaveBeenCalled();
  expect(next.isCurrent()).toBe(true);
  host.dispose();
});
it("parent disposal clears timer and invalid duration or diagnostic is rejected", () => {
  const host = new GenerationViewHost(new AbortController().signal, vi.fn()),
    scope = host.begin(1n),
    failed = vi.fn();
  for (const ms of [0, -1, Infinity, 1.5])
    expect(() => armViewDeadline(scope, ms, "fit", failed)).toThrow("invalid");
  expect(() =>
    armViewDeadline(scope, 100, "untrusted host content", failed),
  ).toThrow("invalid");
  armViewDeadline(scope, 100, "fit", failed);
  host.dispose();
  vi.advanceTimersByTime(200);
  expect(failed).not.toHaveBeenCalled();
});
