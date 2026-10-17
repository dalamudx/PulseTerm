import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: native }));

import {
  decodeVaultStatus,
  lockWorkspaceVault,
  readVaultStatus,
} from "./workspace-vault";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
afterEach(() => {
  native.mockReset();
  vi.useRealTimers();
});
it("accepts only four frozen states, not authority or arbitrary fields", () => {
  for (const state of ["missing", "locked", "pending", "available"]) {
    const value = decodeVaultStatus({ version: 1, state });
    expect(value.state).toBe(state);
    expect(Object.isFrozen(value)).toBe(true);
  }
  for (const value of [
    null,
    [],
    {},
    { version: 2, state: "locked" },
    { version: 1, state: true },
    { version: 1, state: "unlocked" },
    { version: 1, state: "available", password: "no" },
    { version: 1, state: "available", id: nonce },
  ])
    expect(() => decodeVaultStatus(value)).toThrow("vault.invalid");
});
it("status and lock send only current opaque nonce and never password/ref args", async () => {
  native.mockResolvedValue({ version: 1, state: "locked" });
  const signal = new AbortController().signal;
  await readVaultStatus(nonce, signal);
  await lockWorkspaceVault(nonce, signal);
  expect(native.mock.calls).toEqual([
    ["workspace_vault_status", { nonce }],
    ["workspace_vault_lock", { nonce }],
  ]);
});
it("nil/malformed nonce and already retired page never invoke", async () => {
  const life = new AbortController();
  for (const value of [
    "",
    nonce.toUpperCase(),
    "00000000-0000-0000-0000-000000000000",
  ])
    await expect(readVaultStatus(value, life.signal)).rejects.toThrow(
      "vault.nonce",
    );
  life.abort();
  await expect(readVaultStatus(nonce, life.signal)).rejects.toThrow(
    "vault.cancelled",
  );
  expect(native).not.toHaveBeenCalled();
});
it("page abort discards late status/lock receipt without retry", async () => {
  let finish: (value: unknown) => void = () => {};
  native.mockImplementation(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const life = new AbortController();
  const pending = lockWorkspaceVault(nonce, life.signal);
  life.abort();
  await expect(pending).rejects.toThrow("vault.cancelled");
  finish({ version: 1, state: "available" });
  await Promise.resolve();
  expect(native).toHaveBeenCalledTimes(1);
});
it("uncertain lock timeout releases local wait only and never auto retries", async () => {
  vi.useFakeTimers();
  native.mockImplementation(() => new Promise(() => {}));
  const pending = lockWorkspaceVault(nonce, new AbortController().signal);
  const check = expect(pending).rejects.toThrow("vault.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await check;
  expect(native).toHaveBeenCalledTimes(1);
});
