import { afterEach, expect, it, vi } from "vitest";
import {
  decodeIdentityCatalogue,
  readIdentities,
  saveIdentity,
  validateIdentityChange,
} from "./workspace-identities";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  id = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa";
const wire = () => ({
  version: 1,
  token: nonce,
  revision: "1",
  vaultPresent: true,
  identities: [
    {
      id,
      label: "name",
      username: "user",
      secret: { id: nonce, kind: "ssh_password", hasValue: false },
    },
  ],
});
afterEach(() => vi.useRealTimers());
it("freezes metadata and accepts missing value without granting an unlock status", () => {
  const value = decodeIdentityCatalogue(wire());
  expect(value.identities[0]?.secret.hasValue).toBe(false);
  expect(Object.isFrozen(value.identities[0]?.secret)).toBe(true);
  expect(
    decodeIdentityCatalogue({ ...wire(), vaultPresent: false, identities: [] })
      .vaultPresent,
  ).toBe(false);
});
it("rejects version, unknown fields, role, flags, budgets, duplicate and inconsistent namespace", () => {
  for (const raw of [
    { ...wire(), version: 2 },
    { ...wire(), password: "no" },
    { ...wire(), revision: "01" },
    { ...wire(), revision: "9223372036854775808" },
    { ...wire(), vaultPresent: false },
    { ...wire(), identities: Array(257).fill(wire().identities[0]) },
    { ...wire(), identities: [wire().identities[0], wire().identities[0]] },
    {
      ...wire(),
      identities: [
        {
          ...wire().identities[0],
          secret: { id: nonce, kind: "ssh_key_passphrase", hasValue: false },
        },
      ],
    },
    {
      ...wire(),
      identities: [
        {
          ...wire().identities[0],
          secret: { id: nonce, kind: "ssh_password", hasValue: "true" },
        },
      ],
    },
  ])
    expect(() => decodeIdentityCatalogue(raw)).toThrow("identities.invalid");
});
it("encodes only label/login and opaque identity operation; refs/password/flags rejected", () => {
  expect(
    validateIdentityChange({
      action: "create",
      label: "开发身份",
      username: "user",
    }),
  ).toEqual({ operation: "create", label: "开发身份", username: "user" });
  for (const extra of ["secret", "password", "hasValue", "vaultId", "path"]) {
    expect(() =>
      validateIdentityChange({
        action: "create",
        label: "name",
        username: "user",
        [extra]: "blocked",
      }),
    ).toThrow();
  }
  for (const [label, username] of [
    [" name", "user"],
    ["name", "-option"],
    ["name", "u name"],
    ["na\u202eme", "user"],
  ])
    expect(() =>
      validateIdentityChange({ action: "create", label, username }),
    ).toThrow();
});
it("RPC receives no raw credential fields and cancellation rejects late data", async () => {
  const rpc = vi.fn(async () => wire());
  await saveIdentity(
    nonce,
    nonce,
    { action: "update", id, label: "name", username: "user" },
    new AbortController().signal,
    rpc,
  );
  expect(rpc).toHaveBeenCalledWith("workspace_identity_save", {
    nonce,
    token: nonce,
    change: { operation: "update", id, label: "name", username: "user" },
  });
  let finish: (v: unknown) => void = () => {};
  const slow = vi.fn(
    () =>
      new Promise<unknown>((r) => {
        finish = r;
      }),
  );
  const life = new AbortController();
  const pending = readIdentities(nonce, life.signal, slow);
  const rejected = expect(pending).rejects.toThrow("identities.cancelled");
  life.abort();
  finish(wire());
  await rejected;
});
it("uncertain request times out once without automatic retry", async () => {
  vi.useFakeTimers();
  const rpc = vi.fn(() => new Promise<unknown>(() => {}));
  const pending = saveIdentity(
    nonce,
    nonce,
    { action: "remove", id },
    new AbortController().signal,
    rpc,
  );
  const rejected = expect(pending).rejects.toThrow("identities.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await rejected;
  expect(rpc).toHaveBeenCalledTimes(1);
});
