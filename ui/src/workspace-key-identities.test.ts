import { afterEach, expect, it, vi } from "vitest";
import {
  decodeKeyIdentityCatalogue,
  readKeyIdentities,
  saveKeyIdentity,
  validateKeyIdentityChange,
} from "./workspace-key-identities";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  id = "aaaaaaaa-aaaa-7aaa-8aaa-aaaaaaaaaaaa",
  pass = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
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
      secret: { id: nonce, kind: "ssh_private_key", hasValue: false },
      passphrase: { id: pass, kind: "ssh_key_passphrase", hasValue: false },
    },
  ],
});
const row = () => wire().identities[0];
afterEach(() => vi.useRealTimers());
it("independently freezes both drawing references and accepts absent passphrase without granting use", () => {
  const value = decodeKeyIdentityCatalogue(wire());
  expect(Object.isFrozen(value.identities[0]?.passphrase)).toBe(true);
  expect(Object.isFrozen(value.identities[0]?.secret)).toBe(true);
  expect(Object.isFrozen(value.identities)).toBe(true);
  expect(value.identities[0]?.secret.hasValue).toBe(false);
  expect(value.identities[0]?.passphrase?.hasValue).toBe(false);
  expect(
    decodeKeyIdentityCatalogue({
      ...wire(),
      identities: [{ ...row(), passphrase: null }],
    }).identities[0]?.passphrase,
  ).toBeNull();
});
it("rejects corrupt root versions, flags, unknown fields, oversized catalogs and duplicate identities", () => {
  for (const raw of [
    { ...wire(), version: 2 },
    { ...wire(), key: "never" },
    { ...wire(), token: id },
    { ...wire(), revision: "01" },
    { ...wire(), revision: "9223372036854775808" },
    { ...wire(), vaultPresent: "true" },
    { ...wire(), vaultPresent: false },
    { ...wire(), identities: Array(257).fill(row()) },
    { ...wire(), identities: [row(), row()] },
  ])
    expect(() => decodeKeyIdentityCatalogue(raw)).toThrow();
});
it("rejects exchanged roles, shared secret IDs, noncanonical IDs, credential fields and client presence coercion", () => {
  for (const value of [
    { ...row(), secret: { id: nonce, kind: "ssh_password", hasValue: false } },
    {
      ...row(),
      passphrase: { id: pass, kind: "ssh_private_key", hasValue: false },
    },
    {
      ...row(),
      passphrase: { id: nonce, kind: "ssh_key_passphrase", hasValue: false },
    },
    { ...row(), secret: { id: nonce, kind: "ssh_private_key", hasValue: 1 } },
    {
      ...row(),
      secret: {
        id: nonce,
        kind: "ssh_private_key",
        hasValue: false,
        ciphertext: "never",
      },
    },
    { ...row(), passphrase: "never" },
    { ...row(), id: nonce },
    { ...row(), key: "never" },
    {
      ...row(),
      secret: {
        id: "00000000-0000-0000-0000-000000000000",
        kind: "ssh_private_key",
        hasValue: false,
      },
    },
  ])
    expect(() =>
      decodeKeyIdentityCatalogue({ ...wire(), identities: [value] }),
    ).toThrow();
});
it("creation can select only an independent passphrase role and cannot inject refs or credentials", () => {
  expect(
    validateKeyIdentityChange({
      action: "create",
      label: "name",
      username: "user",
      withPassphrase: true,
    }),
  ).toEqual({
    operation: "create",
    label: "name",
    username: "user",
    withPassphrase: true,
  });
  for (const extra of [
    "secretId",
    "vaultId",
    "privateKey",
    "passphrase",
    "key",
    "hasValue",
    "path",
    "arguments",
  ])
    expect(() =>
      validateKeyIdentityChange({
        action: "create",
        label: "name",
        username: "user",
        withPassphrase: false,
        [extra]: "blocked",
      }),
    ).toThrow();
  for (const bad of ["false", undefined, null, 0])
    expect(() =>
      validateKeyIdentityChange({
        action: "create",
        label: "name",
        username: "user",
        withPassphrase: bad,
      }),
    ).toThrow();
});
it("update cannot change passphrase policy or identity IDs and enforces existing text constraints", () => {
  expect(
    validateKeyIdentityChange({
      action: "update",
      id,
      label: "name",
      username: "user",
    }),
  ).toEqual({ operation: "update", id, label: "name", username: "user" });
  expect(() =>
    validateKeyIdentityChange({
      action: "update",
      id,
      label: "name",
      username: "user",
      withPassphrase: true,
    }),
  ).toThrow();
  expect(() =>
    validateKeyIdentityChange({ action: "remove", id: nonce }),
  ).toThrow();
  for (const [label, username] of [
    [" name", "user"],
    ["name", "-flag"],
    ["name", "two words"],
    ["name", "u\u202eser"],
    ["汉".repeat(86), "user"],
  ])
    expect(() =>
      validateKeyIdentityChange({
        action: "create",
        label,
        username,
        withPassphrase: false,
      }),
    ).toThrow();
});
it("RPC is original nonce plus single snapshot ticket and normalized metadata only", async () => {
  const rpc = vi.fn(async () => wire());
  const signal = new AbortController().signal;
  await readKeyIdentities(nonce, signal, rpc);
  await saveKeyIdentity(
    nonce,
    nonce,
    { action: "create", label: "name", username: "user", withPassphrase: true },
    signal,
    rpc,
  );
  expect(rpc.mock.calls).toEqual([
    ["workspace_key_identities", { nonce }],
    [
      "workspace_key_identity_save",
      {
        nonce,
        token: nonce,
        change: {
          operation: "create",
          label: "name",
          username: "user",
          withPassphrase: true,
        },
      },
    ],
  ]);
});
it("aborted or invalid page scope never starts an RPC", async () => {
  const rpc = vi.fn(async () => wire());
  const life = new AbortController();
  life.abort();
  await expect(readKeyIdentities(nonce, life.signal, rpc)).rejects.toThrow(
    "key-identities.cancelled",
  );
  expect(() =>
    readKeyIdentities(id, new AbortController().signal, rpc),
  ).toThrow();
  expect(() =>
    saveKeyIdentity(
      nonce,
      id,
      { action: "remove", id },
      new AbortController().signal,
      rpc,
    ),
  ).toThrow();
  expect(rpc).not.toHaveBeenCalled();
});
it("cancellation rejects late success without a second read or write", async () => {
  let finish: (value: unknown) => void = () => {};
  const rpc = vi.fn(
    () =>
      new Promise<unknown>((resolve) => {
        finish = resolve;
      }),
  );
  const life = new AbortController();
  const pending = saveKeyIdentity(
    nonce,
    nonce,
    { action: "remove", id },
    life.signal,
    rpc,
  );
  const rejected = expect(pending).rejects.toThrow("key-identities.cancelled");
  life.abort();
  finish(wire());
  await rejected;
  expect(rpc).toHaveBeenCalledTimes(1);
});
it("uncertain save times out once and ignores late success without retry", async () => {
  vi.useFakeTimers();
  let finish: (value: unknown) => void = () => {};
  const rpc = vi.fn(
    () =>
      new Promise<unknown>((resolve) => {
        finish = resolve;
      }),
  );
  const pending = saveKeyIdentity(
    nonce,
    nonce,
    { action: "remove", id },
    new AbortController().signal,
    rpc,
  );
  const rejected = expect(pending).rejects.toThrow("key-identities.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await rejected;
  finish(wire());
  await Promise.resolve();
  expect(rpc).toHaveBeenCalledTimes(1);
});
