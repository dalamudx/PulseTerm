import { afterEach, describe, expect, it, vi } from "vitest";
import { TerminalImeCommitGuard } from "./terminal-ime-commit";

class TestTextarea extends EventTarget {
  value = "";
}
afterEach(() => vi.unstubAllGlobals());
function fixture() {
  vi.stubGlobal("HTMLTextAreaElement", TestTextarea);
  const root = new EventTarget(),
    target = new TestTextarea();
  const life = new AbortController(),
    current = vi.fn(() => true);
  const guard = new TerminalImeCommitGuard(root, life.signal, current);
  const send = (type: string, extra = {}) => {
    const stop = vi.fn();
    guard.handle({
      type,
      target,
      isTrusted: true,
      defaultPrevented: false,
      stopImmediatePropagation: stop,
      ...extra,
    });
    return stop;
  };
  const direct = () => {
    send("keydown", { keyCode: 229 });
    send("input", { inputType: "insertFromComposition" });
  };
  return { root, target, life, current, guard, send, direct };
}

describe("direct WebKit IME commits", () => {
  it("blocks repeated unpaired ends owned by the SDK's direct 229 reader", () => {
    const f = fixture();
    for (let i = 0; i < 3; i++) {
      f.direct();
      expect(f.send("compositionend")).toHaveBeenCalledOnce();
      f.send("keyup");
    }
    f.life.abort();
  });
  it("lets normal paired composition reach the SDK", () => {
    const f = fixture();
    f.send("compositionstart");
    f.direct();
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.life.abort();
  });
  it("never intercepts unknown unpaired ends or ordinary insertText", () => {
    const f = fixture();
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.send("keydown", { keyCode: 229 });
    f.send("input", { inputType: "insertText" });
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.send("input", { inputType: "insertFromComposition" });
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.life.abort();
  });
  for (const boundary of ["keyup", "blur", "keydown"]) {
    it(`clears the direct commit at ${boundary}`, () => {
      const f = fixture();
      f.direct();
      f.send(boundary);
      expect(f.send("compositionend")).not.toHaveBeenCalled();
      f.life.abort();
    });
  }
  it("does not act on untrusted or prevented keys, foreign targets or stale views", () => {
    const f = fixture();
    for (const extra of [{ isTrusted: false }, { defaultPrevented: true }]) {
      f.send("keydown", { keyCode: 229, ...extra });
      f.send("input", { inputType: "insertFromComposition" });
      expect(f.send("compositionend")).not.toHaveBeenCalled();
    }
    f.direct();
    expect(
      f.send("compositionend", { target: new TestTextarea() }),
    ).not.toHaveBeenCalled();
    f.direct();
    f.current.mockReturnValue(false);
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.current.mockReturnValue(true);
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.life.abort();
  });
  it("cleans listeners and ignores late events after cancellation", () => {
    const f = fixture(),
      remove = vi.spyOn(f.root, "removeEventListener");
    f.direct();
    f.life.abort();
    expect(remove).toHaveBeenCalledTimes(6);
    expect(f.send("compositionend")).not.toHaveBeenCalled();
    f.guard.dispose();
    expect(remove).toHaveBeenCalledTimes(6);
  });
  it("never reads committed text or the textarea value", () => {
    const f = fixture();
    Object.defineProperty(f.target, "value", {
      get() {
        throw new Error("no text access");
      },
    });
    f.direct();
    expect(f.send("compositionend")).toHaveBeenCalledOnce();
    f.life.abort();
  });
});
