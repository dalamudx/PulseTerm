import { expect, it, vi } from "vitest";
import {
  focusedWorkspaceModal,
  WorkspaceModalEscape,
} from "./workspace-modal-escape";

it("focused nested modal closes before its parent and one Escape gesture cannot dismiss both", () => {
  const parent = new Dialog(),
    child = new Dialog(),
    life = new AbortController();
  const closest = vi.fn(() => (child.open ? child : parent));
  const document = {
    activeElement: { closest },
    querySelector: vi.fn(() => parent),
  } as unknown as Document;
  const handler = new WorkspaceModalEscape(life.signal, () =>
    focusedWorkspaceModal(document),
  );
  expect(handler.keydown(event())).toBe(true);
  expect(child.closes).toBe(1);
  expect(parent.open).toBe(true);
  expect(handler.keydown(event({ repeat: true }))).toBe(true);
  expect(parent.open).toBe(true);
  expect(handler.keyup(event())).toBe(true);
  expect(handler.keydown(event())).toBe(true);
  expect(parent.closes).toBe(1);
  expect(closest).toHaveBeenCalledWith("dialog[open]");
});
it("focused modal lookup retains the original fallback when no dialog owns focus", () => {
  const dialog = new Dialog();
  const document = {
    activeElement: null,
    querySelector: vi.fn(() => dialog),
  } as unknown as Document;
  expect(focusedWorkspaceModal(document)).toBe(dialog);
});
function event(extra: object = {}) {
  return {
    key: "Escape",
    isComposing: false,
    repeat: false,
    defaultPrevented: false,
    ctrlKey: false,
    metaKey: false,
    altKey: false,
    shiftKey: false,
    preventDefault: vi.fn(),
    stopImmediatePropagation: vi.fn(),
    ...extra,
  } as unknown as KeyboardEvent;
}
class Dialog extends EventTarget {
  open = true;
  closes = 0;
  close() {
    this.open = false;
    this.closes++;
  }
}
function fixture() {
  const life = new AbortController(),
    dialog = new Dialog();
  let active: Dialog | null = dialog;
  const handler = new WorkspaceModalEscape(
    life.signal,
    () => active as unknown as HTMLDialogElement | null,
  );
  return {
    life,
    dialog,
    handler,
    setActive: (v: Dialog | null) => {
      active = v;
    },
  };
}
it("consumes keydown before cancel/focus restoration and delegates cancellation once", () => {
  const f = fixture(),
    key = event(),
    cancel = vi.fn((e: Event) => {
      expect(key.preventDefault).toHaveBeenCalled();
      e.preventDefault();
      f.dialog.close();
    });
  f.dialog.addEventListener("cancel", cancel);
  expect(f.handler.keydown(key)).toBe(true);
  expect(cancel).toHaveBeenCalledOnce();
  expect(f.dialog.closes).toBe(1);
  expect(key.stopImmediatePropagation).toHaveBeenCalled();
});
it("search Escape bypasses field clearing and native default closes an unhandled modal", () => {
  const f = fixture();
  expect(f.handler.keydown(event())).toBe(true);
  expect(f.dialog.open).toBe(false);
});
it("repeat/keyup after modal close stay consumed and cannot leak into terminal or cancel a new modal", () => {
  const f = fixture();
  f.handler.keydown(event());
  const next = new Dialog();
  f.setActive(next);
  expect(f.handler.keydown(event({ repeat: true }))).toBe(true);
  expect(next.open).toBe(true);
  f.setActive(null);
  const up = event();
  expect(f.handler.keyup(up)).toBe(true);
  expect(up.preventDefault).toHaveBeenCalled();
  expect(f.handler.keydown(event())).toBe(false);
});
it("ordinary terminal Escape, IME and modifier gestures are not reinterpreted", () => {
  const f = fixture();
  for (const extra of [
    { isComposing: true },
    { keyCode: 229 },
    { key: "Process" },
    { ctrlKey: true },
    { metaKey: true },
    { altKey: true },
    { shiftKey: true },
    { defaultPrevented: true },
    { key: "Enter" },
  ])
    expect(f.handler.keydown(event(extra))).toBe(false);
  f.setActive(null);
  expect(f.handler.keydown(event())).toBe(false);
  expect(f.handler.keyup(event())).toBe(false);
  expect(f.dialog.open).toBe(true);
});
it("page exit rejects all old modal controls including a held keyup", () => {
  const f = fixture();
  f.handler.keydown(event());
  f.life.abort();
  expect(f.handler.keyup(event())).toBe(false);
  expect(f.handler.keydown(event())).toBe(false);
});
