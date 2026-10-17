import { expect, it, vi } from "vitest";
import { bindAutoCopy } from "./selection-copy";

function fixture(enabled = true) {
  const view = new EventTarget();
  const doc = Object.assign(new EventTarget(), {
    hidden: false,
    defaultView: view,
  });
  const root = Object.assign(new EventTarget(), {
    ownerDocument: doc,
    contains: vi.fn(() => false),
  });
  const rootHooks = vi.spyOn(root, "addEventListener"),
    docHooks = vi.spyOn(doc, "addEventListener"),
    viewHooks = vi.spyOn(view, "addEventListener");
  const current = vi.fn(() => true),
    selected = vi.fn(() => true),
    copy = vi.fn();
  const dispose = bindAutoCopy(root as unknown as HTMLElement, {
    enabled,
    current,
    selected,
    copy,
  });
  const fire = (type: string, changes: Record<string, unknown> = {}) => {
    const hook = (
      type === "pointerdown" || type === "focusout"
        ? rootHooks
        : type === "blur"
          ? viewHooks
          : docHooks
    ).mock.calls.find(([name]) => name === type)?.[1];
    if (typeof hook === "function")
      hook({
        isTrusted: true,
        pointerId: 1,
        pointerType: "mouse",
        button: 0,
        buttons: 1,
        target: root,
        ...changes,
      } as unknown as Event);
  };
  return { doc, root, current, selected, copy, dispose, fire };
}
it("copies once only after a trusted original gesture, including release outside the terminal", () => {
  const f = fixture();
  f.fire("pointerup");
  expect(f.copy).not.toHaveBeenCalled();
  f.fire("pointerdown");
  f.fire("pointerup", { target: f.doc });
  f.fire("pointerup");
  expect(f.copy).toHaveBeenCalledOnce();
  f.dispose();
});
it.each([
  { isTrusted: false },
  { button: 2 },
  { buttons: 0 },
  { pointerType: "touch" },
  { pointerId: NaN },
  { pointerId: -1 },
  { target: null },
])("rejects unsafe selection starts %j", (changes) => {
  const f = fixture();
  f.fire("pointerdown", changes);
  f.fire("pointerup");
  expect(f.copy).not.toHaveBeenCalled();
  f.dispose();
});
it.each([{ isTrusted: false }, { pointerId: 2 }, { button: 2 }])(
  "rejects substituted release %j",
  (changes) => {
    const f = fixture();
    f.fire("pointerdown");
    f.fire("pointerup", changes);
    expect(f.copy).not.toHaveBeenCalled();
    f.dispose();
  },
);
it("default-off selection and remote selection changes never request clipboard copy", () => {
  const f = fixture(false);
  f.fire("pointerdown");
  f.fire("pointerup");
  expect(f.copy).not.toHaveBeenCalled();
  f.dispose();
});
it.each(["hidden", "retired", "empty", "cancel", "disposed"])(
  "rejects %s after gesture start",
  (state) => {
    const f = fixture();
    f.fire("pointerdown");
    if (state === "hidden") f.doc.hidden = true;
    if (state === "retired") f.current.mockReturnValue(false);
    if (state === "empty") f.selected.mockReturnValue(false);
    if (state === "cancel") f.fire("pointercancel");
    if (state === "disposed") f.dispose();
    f.fire("pointerup");
    expect(f.copy).not.toHaveBeenCalled();
    f.dispose();
  },
);

it.each(["blur", "visibilitychange", "focusout"])(
  "retires an old gesture across %s and allows a new gesture",
  (event) => {
    const f = fixture();
    f.fire("pointerdown");
    f.fire(event);
    f.fire("pointerup");
    expect(f.copy).not.toHaveBeenCalled();
    f.fire("pointerdown");
    f.fire("pointerup");
    expect(f.copy).toHaveBeenCalledOnce();
    f.dispose();
  },
);
