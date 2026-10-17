import { expect, it, vi } from "vitest";
import type { BindingContext } from "./conditions";
import {
  type BindingKeyEvent,
  KEY_SEQUENCE_DEADLINE_MS,
  KeybindingDispatcher,
} from "./dispatcher";

const source = [
  {
    action: "settings",
    keys: "ctrl+shift+k ctrl+shift+s",
    when: "terminalFocus && !broadcast",
  },
  { action: "hosts", keys: "ctrl+shift+s", when: "workspaceFocus" },
];
function key(key = "k", extra: Partial<BindingKeyEvent> = {}): BindingKeyEvent {
  return {
    key,
    ctrlKey: true,
    metaKey: false,
    altKey: false,
    shiftKey: true,
    isComposing: false,
    defaultPrevented: false,
    repeat: false,
    preventDefault: vi.fn(),
    stopImmediatePropagation: vi.fn(),
    ...extra,
  };
}
function fixture(bindings: unknown = source) {
  const life = new AbortController(),
    execute = vi.fn();
  let nonce = "page-a",
    focus = "pane-a:1",
    time = 0,
    available = true;
  let context: BindingContext = {
    workspaceFocus: true,
    terminalFocus: true,
    editableFocus: false,
    modalOpen: false,
    broadcast: false,
  };
  const runtime = new KeybindingDispatcher(bindings, {
    signal: life.signal,
    nonce: () => nonce,
    focus: () => focus,
    context: () => context,
    available: () => available,
    execute,
    now: () => time,
  });
  return {
    runtime,
    life,
    execute,
    setNonce: (v: string) => {
      nonce = v;
    },
    setFocus: (v: string) => {
      focus = v;
    },
    setTime: (v: number) => {
      time = v;
    },
    setAvailable: (v: boolean) => {
      available = v;
    },
    setContext: (v: Partial<BindingContext>) => {
      context = { ...context, ...v };
    },
  };
}
it("two-step completion consumes gestures once and releases survive modifier/focus changes", () => {
  const f = fixture();
  const down = key();
  expect(f.runtime.keydown(down)).toBe(true);
  expect(f.execute).not.toHaveBeenCalled();
  expect(f.runtime.keyup(key("k", { ctrlKey: false, shiftKey: false }))).toBe(
    true,
  );
  f.setTime(799);
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.execute).toHaveBeenCalledExactlyOnceWith("settings");
  f.setFocus("pane-b:1");
  expect(f.runtime.keyup(key("s", { shiftKey: false }))).toBe(true);
  expect(down.preventDefault).toHaveBeenCalled();
});
it("deadline rejects completion without reinterpreting it as a standalone new action", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.setTime(KEY_SEQUENCE_DEADLINE_MS);
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("repeated prefixes do not extend deadline or execute again", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.setTime(799);
  expect(f.runtime.keydown(key("k", { repeat: true }))).toBe(true);
  f.setTime(801);
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("same numeric generation in another pane cannot inherit a captured prefix", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.setFocus("pane-b:1");
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("page change cannot upgrade a captured prefix into new page authority", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.setNonce("page-b");
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("context changes and lost action availability cancel without redispatch", () => {
  for (const change of [
    { broadcast: true },
    { editableFocus: true },
    { modalOpen: true },
    { terminalFocus: false },
  ]) {
    const f = fixture();
    f.runtime.keydown(key());
    f.setContext(change);
    expect(f.runtime.keydown(key("s"))).toBe(false);
    expect(f.execute).not.toHaveBeenCalled();
  }
  const f = fixture();
  f.runtime.keydown(key());
  f.setAvailable(false);
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("IME/AltGraph/consumed input never executes pending commands", () => {
  for (const extra of [
    { isComposing: true },
    { keyCode: 229 },
    { key: "Process" },
    { defaultPrevented: true },
    { getModifierState: () => true },
  ]) {
    const f = fixture();
    f.runtime.keydown(key());
    expect(f.runtime.keydown(key("s", extra))).toBe(false);
    expect(f.execute).not.toHaveBeenCalled();
  }
});
it("unmatched keys remain unconsumed and never replay the swallowed prefix", () => {
  const f = fixture();
  f.runtime.keydown(key());
  const next = key("y", { ctrlKey: false, shiftKey: false });
  expect(f.runtime.keydown(next)).toBe(false);
  expect(next.preventDefault).not.toHaveBeenCalled();
  expect(f.execute).not.toHaveBeenCalled();
});
it("replacement cannot turn old completion into a new table action", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.replace([{ action: "vault", keys: "ctrl+shift+s" }]);
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
  f.runtime.keyup(key("s"));
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.execute).toHaveBeenCalledExactlyOnceWith("vault");
});
it("failed replacement preserves the original valid table", () => {
  const f = fixture();
  expect(() =>
    f.runtime.replace([{ action: "exec", keys: "ctrl+shift+s" }]),
  ).toThrow();
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.execute).toHaveBeenCalledExactlyOnceWith("hosts");
});
it("proactive prefix cancellation cannot dispatch an old completion on focus restoration", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.cancelPrefix();
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("duplicate downs and repeats cannot dispatch a single-stroke action twice", () => {
  const f = fixture();
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.runtime.keydown(key("s", { repeat: true }))).toBe(true);
  expect(f.execute).toHaveBeenCalledOnce();
  f.runtime.keyup(key("s"));
  f.runtime.keydown(key("s"));
  expect(f.execute).toHaveBeenCalledTimes(2);
});
it("abort/dispose rejects all stale handlers and pending completions", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.life.abort();
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.runtime.keyup(key())).toBe(false);
  f.runtime.replace(source);
  expect(f.execute).not.toHaveBeenCalled();
});
it("physical modifier down/up between chord steps preserves the original prefix", () => {
  const f = fixture();
  expect(f.runtime.keydown(key())).toBe(true);
  f.runtime.keyup(key());
  for (const modifier of ["Control", "Shift", "Meta", "Alt", "OS"]) {
    const event = key(modifier);
    expect(f.runtime.keydown(event)).toBe(false);
    expect(event.preventDefault).not.toHaveBeenCalled();
    f.runtime.keyup(event);
  }
  expect(f.runtime.keydown(key("s"))).toBe(true);
  expect(f.execute).toHaveBeenCalledExactlyOnceWith("settings");
});
it("modifier transitions never extend the eight-hundred-millisecond deadline", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.keyup(key());
  f.setTime(799);
  f.runtime.keydown(key("Control"));
  f.runtime.keydown(key("Shift"));
  f.setTime(800);
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("modifier transitions cannot erase a retired completion after source replacement", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.keyup(key());
  f.runtime.replace([
    { action: "hosts", keys: "ctrl+shift+s", when: "workspaceFocus" },
  ]);
  f.runtime.keydown(key("Control"));
  f.runtime.keydown(key("Shift"));
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});

it("focus round trips proactively cancel the old completion even after context returns", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.keyup(key());
  f.setFocus("pane-b:1");
  f.runtime.cancelPrefix();
  f.setFocus("pane-a:1");
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
it("invalidated source retains its old completion guard when a new table is installed", () => {
  const f = fixture();
  f.runtime.keydown(key());
  f.runtime.keyup(key());
  f.runtime.replace([]);
  f.runtime.replace([
    { action: "hosts", keys: "ctrl+shift+s", when: "workspaceFocus" },
  ]);
  f.runtime.keydown(key("Control"));
  f.runtime.keydown(key("Shift"));
  expect(f.runtime.keydown(key("s"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});

it("non-monotonic or nonfinite clock fails closed", () => {
  for (const clock of [-1, NaN, Infinity]) {
    const f = fixture();
    f.runtime.keydown(key());
    f.setTime(clock);
    expect(f.runtime.keydown(key("s"))).toBe(false);
    expect(f.execute).not.toHaveBeenCalled();
  }
});

it("find only owns its modified terminal gesture and preserves Ctrl+F for the shell", () => {
  const bindings = ["ctrl+shift+f", "meta+shift+f"].map((keys) => ({
    action: "find",
    keys,
    when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
  }));
  for (const meta of [false, true]) {
    const f = fixture(bindings);
    const event = key("f", { ctrlKey: !meta, metaKey: meta });
    expect(f.runtime.keydown(event)).toBe(true);
    expect(f.execute).toHaveBeenCalledExactlyOnceWith("find");
  }
  for (const context of [
    { terminalFocus: false },
    { editableFocus: true },
    { modalOpen: true },
    { broadcast: true },
    { workspaceFocus: false },
  ]) {
    const f = fixture(bindings);
    f.setContext(context);
    const event = key("f");
    expect(f.runtime.keydown(event)).toBe(false);
    expect(event.preventDefault).not.toHaveBeenCalled();
    expect(f.execute).not.toHaveBeenCalled();
  }
  const f = fixture(bindings),
    event = key("f", { shiftKey: false });
  expect(f.runtime.keydown(event)).toBe(false);
  expect(event.preventDefault).not.toHaveBeenCalled();
});
it("find yields to IME, repeats and disabled sources without fallback", () => {
  const bindings = [
    { action: "find", keys: "ctrl+shift+f", when: "terminalFocus" },
  ];
  for (const change of [
    { isComposing: true },
    { keyCode: 229 },
    { key: "Process" },
    { repeat: true },
    { defaultPrevented: true },
  ]) {
    const f = fixture(bindings),
      event = key("f", change);
    expect(f.runtime.keydown(event)).toBe(false);
    expect(f.execute).not.toHaveBeenCalled();
  }
  const f = fixture(bindings);
  f.runtime.replace([]);
  expect(f.runtime.keydown(key("f"))).toBe(false);
  expect(f.execute).not.toHaveBeenCalled();
});
