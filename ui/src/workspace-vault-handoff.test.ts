import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: native }));

import {
  decodeVaultTicket,
  performVaultHandoff,
} from "./workspace-vault-handoff";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const id = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
const ticket = { version: 1, token: id, kind: "unlock" };
afterEach(() => {
  native.mockReset();
  vi.useRealTimers();
});
it("strict fixed ticket rejects extra/version/kind/raw references", () => {
  expect(Object.isFrozen(decodeVaultTicket(ticket))).toBe(true);
  for (const value of [
    { ...ticket, version: 2 },
    { ...ticket, token: nonce },
    { ...ticket, kind: true },
    { ...ticket, secret: "no" },
    null,
    [],
  ])
    expect(() => decodeVaultTicket(value)).toThrow("vault.ticket_invalid");
});
it("native pending requires current frontend confirmation, not auth success alone", async () => {
  native.mockImplementation((command) =>
    Promise.resolve(
      command === "workspace_vault_prepare"
        ? ticket
        : { version: 1, state: "available" },
    ),
  );
  const submit = vi.fn().mockResolvedValue({ version: 1, state: "pending" });
  expect(
    (
      await performVaultHandoff(
        "unlock",
        nonce,
        new AbortController().signal,
        submit,
      )
    ).state,
  ).toBe("available");
  expect(native.mock.calls).toEqual([
    ["workspace_vault_prepare", { nonce, kind: "unlock" }],
    ["workspace_vault_confirm", { nonce, token: id }],
  ]);
});
it("create stays locked without invoking confirm or reading secrets", async () => {
  native.mockResolvedValue({ ...ticket, kind: "create" });
  expect(
    (
      await performVaultHandoff(
        "create",
        nonce,
        new AbortController().signal,
        async () => ({ version: 1, state: "locked" }),
      )
    ).state,
  ).toBe("locked");
  expect(native).toHaveBeenCalledTimes(1);
});
it("late preparation after abort is explicitly cancelled, never submits", async () => {
  let resolve: (v: unknown) => void = () => {};
  native.mockImplementation((c) =>
    c === "workspace_vault_prepare"
      ? new Promise((r) => {
          resolve = r;
        })
      : Promise.resolve(),
  );
  const life = new AbortController();
  const submit = vi.fn();
  const pending = performVaultHandoff("unlock", nonce, life.signal, submit);
  life.abort();
  await expect(pending).rejects.toThrow("vault.cancelled");
  resolve(ticket);
  await Promise.resolve();
  await Promise.resolve();
  expect(submit).not.toHaveBeenCalled();
  expect(native.mock.calls.at(-1)).toEqual([
    "workspace_vault_cancel",
    { nonce, token: id },
  ]);
});
it("submit abort sends explicit native cancel and late auth cannot confirm", async () => {
  native.mockResolvedValue(ticket);
  let resolve: (v: unknown) => void = () => {};
  let entered: () => void = () => {};
  const started = new Promise<void>((r) => {
    entered = r;
  });
  const life = new AbortController();
  const pending = performVaultHandoff("unlock", nonce, life.signal, () => {
    entered();
    return new Promise((r) => {
      resolve = r;
    });
  });
  await started;
  life.abort();
  await expect(pending).rejects.toThrow("vault.cancelled");
  resolve({ version: 1, state: "pending" });
  await Promise.resolve();
  expect(native.mock.calls.map((c) => c[0])).toEqual([
    "workspace_vault_prepare",
    "workspace_vault_cancel",
  ]);
});
it("uncertain submit timeout cancels once and never retries", async () => {
  vi.useFakeTimers();
  native.mockResolvedValue(ticket);
  const submit = vi.fn(() => new Promise(() => {}));
  const pending = performVaultHandoff(
    "unlock",
    nonce,
    new AbortController().signal,
    submit,
  );
  const check = expect(pending).rejects.toThrow("vault.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await check;
  expect(submit).toHaveBeenCalledTimes(1);
  expect(native.mock.calls.map((c) => c[0])).toEqual([
    "workspace_vault_prepare",
    "workspace_vault_cancel",
  ]);
});
it("unknown ready rejects and cancels, cannot become available", async () => {
  native.mockResolvedValue(ticket);
  await expect(
    performVaultHandoff(
      "unlock",
      nonce,
      new AbortController().signal,
      async () => ({ version: 1, state: "available" }),
    ),
  ).rejects.toThrow("vault.ready_invalid");
  expect(native.mock.calls.at(-1)).toEqual([
    "workspace_vault_cancel",
    { nonce, token: id },
  ]);
});
it("abort after confirm dispatch is uncertain, not proof of rollback or a later-key lock", async () => {
  let entered: () => void = () => {};
  const sent = new Promise<void>((r) => {
    entered = r;
  });
  native.mockImplementation((c) => {
    if (c === "workspace_vault_prepare") return Promise.resolve(ticket);
    if (c === "workspace_vault_confirm") {
      entered();
      return new Promise(() => {});
    }
    return Promise.resolve();
  });
  const life = new AbortController();
  const pending = performVaultHandoff(
    "unlock",
    nonce,
    life.signal,
    async () => ({ version: 1, state: "pending" }),
  );
  await sent;
  life.abort();
  await expect(pending).rejects.toThrow("vault.confirm_uncertain");
  expect(native.mock.calls.map((c) => c[0])).toEqual([
    "workspace_vault_prepare",
    "workspace_vault_confirm",
    "workspace_vault_cancel",
  ]);
});
