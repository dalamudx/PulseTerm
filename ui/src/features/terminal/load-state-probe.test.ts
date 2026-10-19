import { invoke } from "@tauri-apps/api/core";
import { afterEach, expect, test, vi } from "vitest";
import { LoadStateProbe } from "./load-state-probe";
import type { DrawnBoundary } from "./terminal-view-stream";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const rpc = vi.mocked(invoke);
const boundary: DrawnBoundary = {
  kind: "marker",
  generation: 2n,
  revision: 3n,
  sourceBytes: 1073741824n,
  utf8Bytes: 1073741825n,
  columns: 80,
  rows: 24,
  digest: "0123456789abcdef",
};
afterEach(() => {
  vi.resetAllMocks();
  vi.unstubAllGlobals();
});
class TestElement extends EventTarget {
  contains(value: unknown): boolean {
    return value === this;
  }
}
function fixture() {
  vi.stubGlobal("Node", TestElement);
  const element = new TestElement() as unknown as HTMLElement;
  const controller = new AbortController();
  const failed = vi.fn();
  const current = vi.fn(() => true);
  const probe = new LoadStateProbe({
    element,
    signal: controller.signal,
    isCurrent: current,
    nonce: "original-page",
    connection: "fixed-view",
    failed,
  });
  const key = {
    target: element,
    isTrusted: true,
    code: "F8",
    ctrlKey: true,
    shiftKey: true,
    altKey: false,
    metaKey: false,
    repeat: false,
    isComposing: false,
    preventDefault: vi.fn(),
    stopImmediatePropagation: vi.fn(),
  };
  return { probe, controller, failed, current, key };
}
test("only an explicitly requested drawn Marker supplies comparison metadata", async () => {
  const f = fixture();
  rpc.mockResolvedValue(undefined);
  await f.probe.observe(boundary);
  expect(rpc).not.toHaveBeenCalled();
  await f.probe.key(f.key);
  await f.probe.key(f.key);
  expect(rpc).toHaveBeenCalledTimes(1);
  await f.probe.observe({ ...boundary, kind: "capture" });
  expect(rpc).toHaveBeenCalledTimes(1);
  await f.probe.observe(boundary);
  expect(rpc).toHaveBeenLastCalledWith("workspace_terminal_load_compare", {
    nonce: "original-page",
    connection: "fixed-view",
    rendered: {
      columns: 80,
      rows: 24,
      revision: "3",
      sourceBytes: "1073741824",
      utf8Bytes: "1073741825",
      digest: "0123456789abcdef",
    },
  });
  await f.probe.observe(boundary);
  expect(rpc).toHaveBeenCalledTimes(2);
  expect(f.key.preventDefault).toHaveBeenCalledOnce();
  f.probe.dispose();
});
test("untrusted, wrong, repeated, composing and foreign keys cannot request a Marker", async () => {
  const f = fixture();
  for (const change of [
    { isTrusted: false },
    { code: "F9" },
    { ctrlKey: false },
    { shiftKey: false },
    { altKey: true },
    { metaKey: true },
    { repeat: true },
    { isComposing: true },
    { target: new TestElement() },
  ])
    await f.probe.key({ ...f.key, ...change });
  expect(rpc).not.toHaveBeenCalled();
  f.current.mockReturnValue(false);
  await f.probe.key(f.key);
  expect(rpc).not.toHaveBeenCalled();
  f.probe.dispose();
});
test("abort removes the listener and rejects late Marker observations and failed receipts", async () => {
  const f = fixture();
  let reject!: (reason: Error) => void;
  rpc.mockImplementationOnce(
    () =>
      new Promise((_resolve, rejectPromise) => {
        reject = rejectPromise;
      }),
  );
  const pending = f.probe.key(f.key);
  f.controller.abort();
  reject(new Error("late native failure"));
  await pending;
  await f.probe.observe(boundary);
  await f.probe.key(f.key);
  expect(rpc).toHaveBeenCalledTimes(1);
  expect(f.failed).not.toHaveBeenCalled();
});
test("a native admission failure fails the current view rather than creating a comparison", async () => {
  const f = fixture();
  rpc.mockRejectedValue(new Error("not admitted"));
  await f.probe.key(f.key);
  expect(f.failed).toHaveBeenCalledOnce();
  await f.probe.observe(boundary);
  expect(rpc).toHaveBeenCalledTimes(1);
  f.probe.dispose();
});
