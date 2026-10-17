import { expect, it, vi } from "vitest";
import { CompositionGuard, isCompositionKey } from "./composition";

const key = { key: "Enter", isComposing: false };
function fixture() {
  const target = new EventTarget(),
    life = new AbortController(),
    started = vi.fn();
  const guard = new CompositionGuard(target, life.signal, started);
  return { target, life, started, guard };
}
it("recognizes explicit composition, WebKit 229 and Process without capturing ordinary keys", () => {
  expect(isCompositionKey(key)).toBe(false);
  for (const event of [
    { ...key, isComposing: true },
    { ...key, keyCode: 229 },
    { ...key, key: "Process" },
  ])
    expect(isCompositionKey(event)).toBe(true);
});
it("candidate keys stay owned by IME even when the browser omits flags", () => {
  const f = fixture();
  const start = new Event("compositionstart", { cancelable: true });
  f.target.dispatchEvent(start);
  for (const value of ["Enter", "Escape", "ArrowUp", "ArrowDown", "c", "r"])
    expect(f.guard.handles({ ...key, key: value })).toBe(true);
  expect(start.defaultPrevented).toBe(false);
  expect(f.started).toHaveBeenCalledOnce();
  f.life.abort();
});
it("end allows the next ordinary gesture while the final 229 key remains IME-owned", () => {
  const f = fixture();
  f.target.dispatchEvent(new Event("compositionstart"));
  f.target.dispatchEvent(new Event("compositionend"));
  expect(f.guard.handles({ ...key, keyCode: 229 })).toBe(true);
  expect(f.guard.handles(key)).toBe(false);
  f.life.abort();
});
it.each(["blur", "pagehide"])(
  "%s clears unfinished composition without replaying a commit",
  (event) => {
    const f = fixture();
    f.target.dispatchEvent(new Event("compositionstart"));
    f.target.dispatchEvent(new Event(event));
    expect(f.guard.handles(key)).toBe(false);
    f.life.abort();
  },
);
it("aborting detaches listeners, forgets composition and ignores late events", () => {
  const f = fixture();
  const remove = vi.spyOn(f.target, "removeEventListener");
  f.target.dispatchEvent(new Event("compositionstart"));
  f.life.abort();
  f.guard.dispose();
  f.target.dispatchEvent(new Event("compositionstart"));
  expect(f.started).toHaveBeenCalledOnce();
  expect(f.guard.handles({ ...key, isComposing: true })).toBe(false);
  expect(remove).toHaveBeenCalledTimes(4);
});
it("guards never install listeners on a retired input scope", () => {
  const life = new AbortController();
  life.abort();
  const target = new EventTarget(),
    add = vi.spyOn(target, "addEventListener");
  const guard = new CompositionGuard(target, life.signal);
  expect(add).not.toHaveBeenCalled();
  expect(guard.handles(key)).toBe(false);
});
it("different roots do not share candidate state", () => {
  const first = fixture(),
    second = fixture();
  first.target.dispatchEvent(new Event("compositionstart"));
  expect(first.guard.handles(key)).toBe(true);
  expect(second.guard.handles(key)).toBe(false);
  first.life.abort();
  second.life.abort();
});
