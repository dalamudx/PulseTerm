import { expect, it, vi } from "vitest";
import {
  NativeVisibilityBuffer,
  NativeVisibilityFilter,
  nativeShown,
} from "./native-visibility";
import { SurfaceVisibility } from "./surface-visibility";

const sample = {
  incarnation: "window-incarnation",
  revision: 1,
  visible: true,
  minimized: false,
  valid: true,
};
it("bootstrap keeps the highest revision per identity despite reordered foreign events", () => {
  const b = new NativeVisibilityBuffer();
  b.add({ ...sample, revision: 4, minimized: true });
  b.add({ ...sample, revision: 3 });
  b.add({ ...sample, incarnation: "old", revision: 100 });
  const buffered = b.take(sample.incarnation);
  expect(buffered?.revision).toBe(4);
  const f = new NativeVisibilityFilter(sample);
  expect(buffered && f.accept(buffered)?.minimized).toBe(true);
  expect(b.take(sample.incarnation)).toBeUndefined();
});
it("bootstrap buffer is bounded and overflow fails rather than silently losing current state", () => {
  const b = new NativeVisibilityBuffer();
  for (let i = 0; i < 4; i++) b.add({ ...sample, incarnation: `window-${i}` });
  expect(() => b.add({ ...sample, incarnation: "extra" })).toThrow("overload");
  expect(b.take("window-0")?.visible).toBe(true);
  b.add(sample);
  expect(b.take(sample.incarnation)).toEqual(sample);
});
it("bootstrap validates flags and cannot store a caller-mutated stale reference", () => {
  const b = new NativeVisibilityBuffer();
  const s = { ...sample };
  b.add(s);
  s.visible = false;
  expect(b.take(sample.incarnation)?.visible).toBe(true);
  expect(() => b.add({ ...sample, revision: 0 })).toThrow("invalid");
});

it("ignores old incarnation, duplicated and reordered samples", () => {
  const f = new NativeVisibilityFilter(sample);
  expect(f.accept(sample)).toBeNull();
  expect(f.accept({ ...sample, incarnation: "old", revision: 90 })).toBeNull();
  expect(f.accept({ ...sample, revision: 3, minimized: true })?.minimized).toBe(
    true,
  );
  expect(f.accept({ ...sample, revision: 2 })).toBeNull();
  expect(f.accept({ ...sample, revision: 4 })?.visible).toBe(true);
});
it("current query failures propagate invalid instead of fabricating visible evidence", () => {
  const f = new NativeVisibilityFilter(sample);
  expect(f.accept({ ...sample, revision: 2, valid: false })?.valid).toBe(false);
  expect(nativeShown({ ...sample, valid: false })).toBe(false);
});
it("requires valid bounded integer revision and snapshot", () => {
  for (const revision of [0, -1, 0x100000000, NaN, 1.5])
    expect(() => new NativeVisibilityFilter({ ...sample, revision })).toThrow(
      "invalid",
    );
  expect(() => new NativeVisibilityFilter({ ...sample, valid: false })).toThrow(
    "snapshot",
  );
  expect(
    () => new NativeVisibilityFilter({ ...sample, incarnation: "" }),
  ).toThrow("invalid");
});
it("runtime focus hints cannot imply hidden and minimized does not require !visible", () => {
  expect(nativeShown(sample)).toBe(true);
  expect(nativeShown({ ...sample, minimized: true })).toBe(false);
  expect(nativeShown({ ...sample, visible: false })).toBe(false);
});
it("native false alone retires and pauses while DOM remains visible", async () => {
  const actions = {
    retire: vi.fn(),
    pause: vi.fn().mockResolvedValue(undefined),
    resume: vi.fn().mockResolvedValue(undefined),
    failed: vi.fn(),
  };
  const view = new SurfaceVisibility(actions, new AbortController().signal);
  view.observe("native", false);
  expect(actions.retire).toHaveBeenCalledTimes(1);
  await view.paused;
  view.observeSnapshot({ container: true, document: true });
  await Promise.resolve();
  expect(actions.resume).not.toHaveBeenCalled();
  view.observe("native", true);
  await view.resumed;
  expect(actions.resume).toHaveBeenCalledTimes(1);
});
it("native restore still cannot bypass a hidden document", async () => {
  const actions = {
    retire: vi.fn(),
    pause: vi.fn().mockResolvedValue(undefined),
    resume: vi.fn().mockResolvedValue(undefined),
    failed: vi.fn(),
  };
  const view = new SurfaceVisibility(actions, new AbortController().signal);
  view.observeSnapshot({ native: false, document: false });
  await view.paused;
  view.observe("native", true);
  await Promise.resolve();
  expect(actions.resume).not.toHaveBeenCalled();
  view.observe("document", true);
  await view.resumed;
  expect(actions.resume).toHaveBeenCalledTimes(1);
});
