import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ConfigSourceMonitor } from "./workspace-ssh-config-monitor";

const nonce = "11111111-1111-4111-8111-111111111111";
const token = "22222222-2222-4222-8222-222222222222";
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((yes) => {
    resolve = yes;
  });
  return { promise, resolve };
}
function fixture() {
  const ports = {
    check: vi.fn(
      async (
        _nonce: string,
        _token: string,
        _signal: AbortSignal,
      ): Promise<"unchanged" | "changed"> => "unchanged",
    ),
    visible: vi.fn(() => true),
    checking: vi.fn(),
    invalid: vi.fn(),
  };
  const current = vi.fn(() => true);
  const monitor = new ConfigSourceMonitor(ports);
  return { ports, current, monitor };
}
beforeEach(() => vi.useFakeTimers());
afterEach(() => {
  vi.clearAllTimers();
  vi.useRealTimers();
});
describe("source monitor preserves the original preview scope", () => {
  it("reuses original nonce/token without choosing a new target", async () => {
    const f = fixture();
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(6000);
    expect(f.ports.check).toHaveBeenCalledTimes(3);
    for (const call of f.ports.check.mock.calls)
      expect(call.slice(0, 2)).toEqual([nonce, token]);
    expect(f.ports.invalid).not.toHaveBeenCalled();
    f.monitor.stop();
  });
  it("does not enqueue overlapping checks", async () => {
    const f = fixture(),
      pending = deferred<"unchanged">();
    f.ports.check.mockReturnValueOnce(pending.promise);
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(20000);
    expect(f.ports.check).toHaveBeenCalledTimes(1);
    pending.resolve("unchanged");
    await vi.advanceTimersByTimeAsync(2000);
    expect(f.ports.check).toHaveBeenCalledTimes(2);
    f.monitor.stop();
  });
  it("skips I/O in hidden pages and resumes only while current", async () => {
    const f = fixture();
    f.ports.visible.mockReturnValue(false);
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(6000);
    expect(f.ports.check).not.toHaveBeenCalled();
    f.ports.visible.mockReturnValue(true);
    await vi.advanceTimersByTimeAsync(2000);
    expect(f.ports.check).toHaveBeenCalledTimes(1);
    f.monitor.stop();
  });
  it("aborts a stopped check and ignores its late change result", async () => {
    const f = fixture(),
      pending = deferred<"changed">();
    f.ports.check.mockReturnValueOnce(pending.promise);
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(2000);
    const signal = f.ports.check.mock.calls[0]?.[2];
    f.monitor.stop();
    expect(signal?.aborted).toBe(true);
    pending.resolve("changed");
    await vi.advanceTimersByTimeAsync(6000);
    expect(f.ports.invalid).not.toHaveBeenCalled();
    expect(f.ports.check).toHaveBeenCalledTimes(1);
  });
  it("ignores old-page completion and retires future checks", async () => {
    const f = fixture(),
      pending = deferred<"changed">();
    f.ports.check.mockReturnValueOnce(pending.promise);
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(2000);
    f.current.mockReturnValue(false);
    pending.resolve("changed");
    await vi.advanceTimersByTimeAsync(6000);
    expect(f.ports.invalid).not.toHaveBeenCalled();
    expect(f.ports.check).toHaveBeenCalledTimes(1);
    f.monitor.stop();
  });
  it("reports change once and stops reading", async () => {
    const f = fixture();
    f.ports.check.mockResolvedValue("changed");
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(10000);
    expect(f.ports.invalid.mock.calls).toEqual([["changed"]]);
    expect(f.ports.check).toHaveBeenCalledTimes(1);
    f.monitor.stop();
  });
  it("does not mistake temporarily held native credit for source change", async () => {
    const f = fixture();
    f.ports.check.mockRejectedValueOnce("workspace.busy");
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(4000);
    expect(f.ports.check).toHaveBeenCalledTimes(2);
    expect(f.ports.invalid).not.toHaveBeenCalled();
    f.monitor.stop();
  });
  it("does not automatically retry uncertain checks or prolong authorization", async () => {
    const f = fixture();
    f.ports.check.mockRejectedValueOnce(Error("config.timeout"));
    f.monitor.start(nonce, token, f.current);
    await vi.advanceTimersByTimeAsync(10000);
    expect(f.ports.invalid.mock.calls).toEqual([["unavailable"]]);
    expect(f.ports.check).toHaveBeenCalledTimes(1);
    f.monitor.stop();
  });
});
