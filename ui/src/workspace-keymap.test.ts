import { afterEach, expect, it, vi } from "vitest";
import { workspaceKeybindingCommandIds } from "./workspace-command-catalog";
import {
  confirmWorkspaceKeymap,
  decodeWorkspaceKeymap,
  type KeymapRpc,
  readWorkspaceKeymap,
  saveWorkspaceKeymap,
  validateKeymapChange,
} from "./workspace-keymap";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  first = "11111111-1111-4111-8111-111111111111",
  second = "22222222-2222-4222-8222-222222222222",
  third = "33333333-3333-4333-8333-333333333333";
function wire(token = first, confirmed = false) {
  return {
    version: 1,
    token,
    confirmed,
    values: {
      version: 1,
      bindings: [
        {
          action: "find",
          keys: "ctrl+shift+f",
          when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
        },
        {
          action: "find",
          keys: "meta+shift+f",
          when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
        },
        {
          action: "palette",
          keys: "ctrl+shift+p",
          when: "workspaceFocus && !editableFocus && !modalOpen",
        },
        {
          action: "palette",
          keys: "meta+shift+p",
          when: "workspaceFocus && !editableFocus && !modalOpen",
        },
      ],
      sources: [...workspaceKeybindingCommandIds, "palette"].map((action) => ({
        action,
        source: "default",
        disabled: action !== "palette" && action !== "find",
      })),
    },
  };
}
const change = {
  kind: "set",
  action: "settings",
  keys: ["ctrl+shift+s"],
  when: "workspaceFocus && !editableFocus && !modalOpen",
} as const;
afterEach(() => vi.useRealTimers());
it("decodes exact immutable source data and preserves compiled projection identity", () => {
  const decoded = decodeWorkspaceKeymap(wire());
  expect(Object.isFrozen(decoded)).toBe(true);
  expect(decodeWorkspaceKeymap(decoded)).toBe(decoded);
  expect(decoded.values.bindings[0]?.sequence).toHaveLength(1);
});
it("read confirm save require distinct staged tickets and send only fixed metadata", async () => {
  const rpc = vi.fn(async (command: string) =>
    command === "workspace_keymap_read"
      ? wire()
      : command === "workspace_keymap_confirm"
        ? wire(second, true)
        : wire(third, false),
  );
  const signal = new AbortController().signal;
  const candidate = await readWorkspaceKeymap(nonce, signal, rpc);
  const accepted = await confirmWorkspaceKeymap(nonce, candidate, signal, rpc);
  const saved = await saveWorkspaceKeymap(
    nonce,
    accepted,
    change,
    true,
    signal,
    rpc,
  );
  expect(saved.confirmed).toBe(false);
  expect(rpc.mock.calls.map((call) => call[0])).toEqual([
    "workspace_keymap_read",
    "workspace_keymap_confirm",
    "workspace_keymap_save",
  ]);
  expect(rpc).toHaveBeenLastCalledWith("workspace_keymap_save", {
    nonce,
    ticket: second,
    change,
    consent: true,
  });
});
it("read-only source or missing rewrite consent cannot send a write", async () => {
  const rpc = vi.fn(async () => wire());
  const signal = new AbortController().signal;
  await expect(
    saveWorkspaceKeymap(
      nonce,
      decodeWorkspaceKeymap(wire()),
      change,
      true,
      signal,
      rpc,
    ),
  ).rejects.toThrow("invalid_phase");
  await expect(
    saveWorkspaceKeymap(
      nonce,
      decodeWorkspaceKeymap(wire(second, true)),
      change,
      false,
      signal,
      rpc,
    ),
  ).rejects.toThrow("invalid_phase");
  expect(rpc).not.toHaveBeenCalled();
});
it("bad nonce/candidate phase stops before native dispatch", async () => {
  const rpc = vi.fn();
  const signal = new AbortController().signal;
  await expect(readWorkspaceKeymap("wrong", signal, rpc)).rejects.toThrow();
  await expect(
    confirmWorkspaceKeymap(
      nonce,
      decodeWorkspaceKeymap(wire(second, true)),
      signal,
      rpc,
    ),
  ).rejects.toThrow("invalid_phase");
  expect(rpc).not.toHaveBeenCalled();
});
it("malformed versions, fields, sources and ticket formats reject as a whole", () => {
  for (const value of [
    null,
    {},
    { ...wire(), version: 2 },
    { ...wire(), token: nonce.toUpperCase() },
    { ...wire(), permission: true },
    { ...wire(), confirmed: 1 },
    { ...wire(), values: {} },
  ])
    expect(() => decodeWorkspaceKeymap(value)).toThrow();
});
it("unknown actions, fields, conditions and aliases cannot be submitted", () => {
  for (const value of [
    null,
    { ...change, action: "exec" },
    { ...change, path: "/bin/sh" },
    { ...change, keys: ["ctrl+p"] },
    { ...change, keys: ["ctrl+shift+s", "ctrl+shift+s"] },
    { ...change, keys: Array(5).fill("ctrl+shift+s") },
    { ...change, when: "window.focus()" },
    { kind: "restore", action: "settings", when: "" },
  ])
    expect(() => validateKeymapChange(value)).toThrow();
});
it("disable and restore are explicit bounded metadata changes", () => {
  expect(validateKeymapChange({ ...change, keys: [] })).toEqual({
    ...change,
    keys: [],
  });
  expect(validateKeymapChange({ kind: "restore", action: "settings" })).toEqual(
    { kind: "restore", action: "settings" },
  );
});
it("confirmation rejects old-ticket reuse or an unconfirmed response and revokes once", async () => {
  for (const response of [wire(first, true), wire(second, false)]) {
    const rpc = vi.fn(async (_command: string) => response);
    await expect(
      confirmWorkspaceKeymap(
        nonce,
        decodeWorkspaceKeymap(wire()),
        new AbortController().signal,
        rpc,
      ),
    ).rejects.toThrow("invalid_phase");
    expect(
      rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_cancel"),
    ).toHaveLength(1);
  }
});
it("save receipt cannot directly enable bindings and malformed phase triggers cancellation", async () => {
  const rpc = vi.fn(async (_command: string) => wire(third, true));
  await expect(
    saveWorkspaceKeymap(
      nonce,
      decodeWorkspaceKeymap(wire(second, true)),
      change,
      true,
      new AbortController().signal,
      rpc,
    ),
  ).rejects.toThrow("invalid_phase");
  expect(
    rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_cancel"),
  ).toHaveLength(1);
});
it("aborted queued request never invokes native or cancels another action", async () => {
  const rpc = vi.fn();
  const life = new AbortController();
  life.abort();
  await expect(readWorkspaceKeymap(nonce, life.signal, rpc)).rejects.toThrow(
    "cancelled",
  );
  expect(rpc).not.toHaveBeenCalled();
});
it("abort pending read revokes exactly once and ignores a late successful receipt", async () => {
  let release: (value: unknown) => void = () => {};
  const rpc = vi.fn((command: string) =>
    command === "workspace_keymap_cancel"
      ? Promise.resolve(null)
      : new Promise((resolve) => {
          release = resolve;
        }),
  );
  const life = new AbortController();
  const pending = readWorkspaceKeymap(nonce, life.signal, rpc);
  life.abort();
  await expect(pending).rejects.toThrow("cancelled");
  release(wire());
  await Promise.resolve();
  expect(
    rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_cancel"),
  ).toHaveLength(1);
});
it("five-second cutoff revokes uncertain writes and never retries them", async () => {
  vi.useFakeTimers();
  const rpc = vi.fn((command: string) =>
    command === "workspace_keymap_cancel"
      ? Promise.resolve(null)
      : new Promise(() => {}),
  );
  const pending = saveWorkspaceKeymap(
    nonce,
    decodeWorkspaceKeymap(wire(second, true)),
    change,
    true,
    new AbortController().signal,
    rpc,
  );
  const outcome = expect(pending).rejects.toThrow("timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await outcome;
  expect(
    rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_save"),
  ).toHaveLength(1);
  expect(
    rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_cancel"),
  ).toHaveLength(1);
});
it("transport or decoder errors revoke the original action without reporting success", async () => {
  const rpc = vi.fn(async (command: string) =>
    command === "workspace_keymap_cancel" ? null : { version: 2 },
  );
  await expect(
    readWorkspaceKeymap(nonce, new AbortController().signal, rpc),
  ).rejects.toThrow();
  expect(
    rpc.mock.calls.filter((call) => call[0] === "workspace_keymap_cancel"),
  ).toHaveLength(1);
});
it("synchronous transport/cancel errors cannot hide the first failure", async () => {
  const rpc: KeymapRpc = () => {
    throw Error("original transport failure");
  };
  await expect(
    readWorkspaceKeymap(nonce, new AbortController().signal, rpc),
  ).rejects.toThrow("original transport failure");
});
it("a late page-scoped cancellation cannot overtake a new source flow", async () => {
  let cancelDone: () => void = () => {};
  let reads = 0;
  const rpc = vi.fn((command: string) => {
    if (command === "workspace_keymap_cancel")
      return new Promise<void>((resolve) => {
        cancelDone = resolve;
      });
    reads++;
    return reads === 1 ? new Promise(() => {}) : Promise.resolve(wire());
  });
  const old = new AbortController();
  const firstRead = readWorkspaceKeymap(nonce, old.signal, rpc);
  old.abort();
  await expect(firstRead).rejects.toThrow("cancelled");
  const next = readWorkspaceKeymap(nonce, new AbortController().signal, rpc);
  await Promise.resolve();
  expect(reads).toBe(1);
  cancelDone();
  await expect(next).resolves.toMatchObject({ confirmed: false });
  expect(reads).toBe(2);
});
it("unconfirmed cancellation blocks subsequent native dispatch until page reload", async () => {
  const rpc = vi.fn((command: string) =>
    command === "workspace_keymap_cancel"
      ? Promise.reject(Error("cancel lost"))
      : Promise.resolve({ version: 2 }),
  );
  await expect(
    readWorkspaceKeymap(nonce, new AbortController().signal, rpc),
  ).rejects.toThrow();
  await Promise.resolve();
  const before = rpc.mock.calls.length;
  await expect(
    readWorkspaceKeymap(nonce, new AbortController().signal, rpc),
  ).rejects.toThrow();
  expect(rpc.mock.calls).toHaveLength(before);
});

it("accessors are rejected without running caller code", () => {
  let invoked = false;
  const value = {
    ...change,
    get kind() {
      invoked = true;
      return "set";
    },
  };
  expect(() => validateKeymapChange(value)).toThrow();
  expect(invoked).toBe(false);
});
