import { afterEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: native }));

import { submitMasterPassword } from "./workspace-vault-password";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  token = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
const ticket = { version: 1, token, kind: "unlock" };
afterEach(() => native.mockReset());
it("owned bytes and wire array clear after explicit submit/confirm without read-secret or arbitrary source", async () => {
  let copied: number[] = [];
  let wire: number[] = [];
  native.mockImplementation((command, args) => {
    if (command === "workspace_vault_prepare") return Promise.resolve(ticket);
    if (command === "workspace_vault_submit") {
      wire = args.password;
      copied = [...wire];
      expect(Object.keys(args).sort()).toEqual([
        "kind",
        "mode",
        "nonce",
        "password",
        "token",
      ]);
      expect(args.mode).toBe("master_password");
      return Promise.resolve({ version: 1, state: "pending" });
    }
    return Promise.resolve({ version: 1, state: "available" });
  });
  const bytes = new Uint8Array([1, 2, 3]);
  expect(
    (
      await submitMasterPassword(
        "unlock",
        nonce,
        bytes,
        new AbortController().signal,
      )
    ).state,
  ).toBe("available");
  expect(copied).toEqual([1, 2, 3]);
  expect([...bytes]).toEqual([0, 0, 0]);
  expect(wire).toEqual([0, 0, 0]);
});
it("malformed nonce/empty/over-budget/preaborted input clear before any native password dispatch", async () => {
  for (const bytes of [
    new Uint8Array(),
    new Uint8Array(4097).fill(42),
    new Uint8Array([42]),
  ]) {
    const controller = new AbortController();
    controller.abort();
    await expect(
      submitMasterPassword("unlock", nonce, bytes, controller.signal),
    ).rejects.toThrow();
    expect([...bytes].every((b) => b === 0)).toBe(true);
  }
  const bytes = new Uint8Array([42]);
  await expect(
    submitMasterPassword("unlock", "bad", bytes, new AbortController().signal),
  ).rejects.toThrow("vault.nonce");
  expect([...bytes]).toEqual([0]);
  expect(native).not.toHaveBeenCalled();
});
it("native rejection clears owned wire, no confirm or automatic retry", async () => {
  let wire: number[] = [];
  native.mockImplementation((command, args) => {
    if (command === "workspace_vault_prepare") return Promise.resolve(ticket);
    if (command === "workspace_vault_submit") {
      wire = args.password;
      return Promise.reject(Error("native.denied"));
    }
    return Promise.resolve();
  });
  const bytes = new Uint8Array([42]);
  await expect(
    submitMasterPassword("unlock", nonce, bytes, new AbortController().signal),
  ).rejects.toThrow("native.denied");
  expect([...bytes]).toEqual([0]);
  expect(wire).toEqual([0]);
  expect(native.mock.calls.map((c) => c[0])).toEqual([
    "workspace_vault_prepare",
    "workspace_vault_submit",
    "workspace_vault_cancel",
  ]);
});
it("abort wipes wire immediately even when native response never returns", async () => {
  let wire: number[] = [];
  let entered: () => void = () => {};
  const ready = new Promise<void>((r) => {
    entered = r;
  });
  native.mockImplementation((command, args) => {
    if (command === "workspace_vault_prepare") return Promise.resolve(ticket);
    if (command === "workspace_vault_submit") {
      wire = args.password;
      entered();
      return new Promise(() => {});
    }
    return Promise.resolve();
  });
  const life = new AbortController(),
    bytes = new Uint8Array([42]);
  const pending = submitMasterPassword("unlock", nonce, bytes, life.signal);
  await ready;
  life.abort();
  expect([...bytes]).toEqual([0]);
  expect(wire).toEqual([0]);
  await expect(pending).rejects.toThrow("vault.cancelled");
  expect(native.mock.calls.map((c) => c[0])).toEqual([
    "workspace_vault_prepare",
    "workspace_vault_submit",
    "workspace_vault_cancel",
  ]);
});
