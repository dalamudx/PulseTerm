import { afterEach, expect, it, vi } from "vitest";
import { PasteGate } from "./paste-policy";
import { bindRightClickPaste } from "./right-click-paste";

afterEach(() => vi.useRealTimers());
function fixture(shared?: Document) {
  const view = new EventTarget();
  const doc =
    shared ??
    (Object.assign(new EventTarget(), {
      hidden: false,
      visibilityState: "visible",
      hasFocus: vi.fn(() => true),
      activeElement: null,
      defaultView: view,
    }) as unknown as Document);
  const root = Object.assign(new EventTarget(), {
    ownerDocument: doc,
    isConnected: true,
    contains: (value: unknown) => value === root,
  });
  Object.assign(doc, { activeElement: root });
  const hooks = vi.spyOn(root, "addEventListener");
  const controller = new AbortController();
  const current = vi.fn(() => true),
    enabled = vi.fn(() => true),
    mouseMode = vi.fn(() => "none"),
    readText = vi.fn(async () => "plain"),
    submit = vi.fn(async (_text: string) => {}),
    rejected = vi.fn();
  const dispose = bindRightClickPaste(root as unknown as HTMLElement, {
    signal: controller.signal,
    current,
    enabled,
    mouseMode,
    focus: () => Object.assign(doc, { activeElement: root }),
    readText,
    submit,
    rejected,
  });
  const fire = (changes: Record<string, unknown> = {}) => {
    const event = {
      isTrusted: true,
      button: 2,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      metaKey: false,
      preventDefault: vi.fn(),
      stopImmediatePropagation: vi.fn(),
      ...changes,
    };
    const hook = hooks.mock.calls.find(([name]) => name === "contextmenu")?.[1];
    if (typeof hook === "function") hook(event as unknown as Event);
    return event;
  };
  return {
    doc,
    root,
    view,
    controller,
    current,
    enabled,
    mouseMode,
    readText,
    submit,
    rejected,
    dispose,
    fire,
  };
}
async function flush() {
  for (let i = 0; i < 6; i++) await Promise.resolve();
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
it("reads once for a trusted right click then submits through the existing gate", async () => {
  const f = fixture();
  expect(f.fire().preventDefault).toHaveBeenCalledOnce();
  await flush();
  expect(f.readText).toHaveBeenCalledOnce();
  expect(f.submit).toHaveBeenCalledExactlyOnceWith("plain");
  f.dispose();
});
it.each([
  { isTrusted: false },
  { button: 0 },
  { button: 1 },
  { shiftKey: true },
  { ctrlKey: true },
  { altKey: true },
  { metaKey: true },
])("does not consume another gesture %j", async (changes) => {
  const f = fixture();
  const event = f.fire(changes);
  await flush();
  expect(event.preventDefault).not.toHaveBeenCalled();
  expect(f.readText).not.toHaveBeenCalled();
  f.dispose();
});
it.each(["x10", "vt200", "drag", "any", "unknown"])(
  "preserves terminal mouse reporting %s",
  async (mode) => {
    const f = fixture();
    f.mouseMode.mockReturnValue(mode);
    expect(f.fire().preventDefault).not.toHaveBeenCalled();
    await flush();
    expect(f.readText).not.toHaveBeenCalled();
    f.dispose();
  },
);
it("default-off, retired, unfocused and hidden surfaces never read", async () => {
  for (const condition of [
    "off",
    "retired",
    "hidden",
    "unfocused",
    "disconnected",
  ]) {
    const f = fixture();
    if (condition === "off") f.enabled.mockReturnValue(false);
    if (condition === "retired") f.current.mockReturnValue(false);
    if (condition === "hidden") Object.assign(f.doc, { hidden: true });
    if (condition === "unfocused")
      vi.mocked(f.doc.hasFocus).mockReturnValue(false);
    if (condition === "disconnected") f.root.isConnected = false;
    expect(f.fire().preventDefault).not.toHaveBeenCalled();
    await flush();
    expect(f.readText).not.toHaveBeenCalled();
    f.dispose();
  }
});
it.each(["navigation", "blur", "visibility", "focusout", "mode"])(
  "rejects a late read after %s",
  async (reason) => {
    const f = fixture(),
      read = deferred<string>();
    f.readText.mockImplementation(() => read.promise);
    f.fire();
    if (reason === "navigation") f.controller.abort();
    if (reason === "blur") f.view.dispatchEvent(new Event("blur"));
    if (reason === "visibility")
      f.doc.dispatchEvent(new Event("visibilitychange"));
    if (reason === "focusout") f.root.dispatchEvent(new Event("focusout"));
    if (reason === "mode") f.mouseMode.mockReturnValue("vt200");
    read.resolve("late");
    await flush();
    expect(f.submit).not.toHaveBeenCalled();
    f.dispose();
  },
);
it("cancellation retains the document slot until the actual read exits", async () => {
  const f = fixture(),
    read = deferred<string>();
  f.readText.mockImplementation(() => read.promise);
  f.fire();
  f.dispose();
  const next = fixture(f.doc);
  next.fire();
  expect(next.readText).not.toHaveBeenCalled();
  expect(next.rejected).toHaveBeenCalledWith("paste.busy");
  read.resolve("retired");
  await flush();
  next.fire();
  await flush();
  expect(f.submit).not.toHaveBeenCalled();
  expect(next.submit).toHaveBeenCalledExactlyOnceWith("plain");
  next.dispose();
});
it("five-second uncertainty never retries and retains credit until exit", async () => {
  vi.useFakeTimers();
  const f = fixture(),
    read = deferred<string>();
  f.readText.mockImplementation(() => read.promise);
  f.fire();
  await vi.advanceTimersByTimeAsync(5000);
  expect(f.rejected).toHaveBeenCalledExactlyOnceWith(
    "paste.clipboard_unavailable",
  );
  f.fire();
  expect(f.readText).toHaveBeenCalledOnce();
  read.resolve("too late");
  await flush();
  expect(f.submit).not.toHaveBeenCalled();
  f.dispose();
});
it("empty or rejected clipboard reads send no input", async () => {
  for (const fails of [false, true]) {
    const f = fixture();
    if (fails) f.readText.mockRejectedValue(Error("private clipboard error"));
    else f.readText.mockResolvedValue("");
    f.fire();
    await flush();
    expect(f.submit).not.toHaveBeenCalled();
    if (fails)
      expect(f.rejected).toHaveBeenCalledExactlyOnceWith(
        "paste.clipboard_unavailable",
      );
    f.dispose();
  }
});
it("multi-line text uses the same approval gate and cancellation writes nothing", async () => {
  const f = fixture();
  const approved = deferred<boolean>(),
    confirm = vi.fn(() => approved.promise),
    send = vi.fn();
  const gate = new PasteGate(f.controller.signal, f.current, confirm, send);
  f.submit.mockImplementation(async (text) => {
    await gate.submit(text);
  });
  f.readText.mockResolvedValue("a\nb");
  f.fire();
  await flush();
  expect(confirm).toHaveBeenCalledOnce();
  expect(send).not.toHaveBeenCalled();
  f.controller.abort();
  approved.resolve(true);
  await flush();
  expect(send).not.toHaveBeenCalled();
  gate.close();
  f.dispose();
});
