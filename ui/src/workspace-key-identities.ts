import { invoke } from "@tauri-apps/api/core";
import {
  type IdentityFields,
  type IdentityRpc,
  validateIdentityChange,
} from "./workspace-identities";
export type KeyIdentity = IdentityFields &
  Readonly<{
    id: string;
    secret: Readonly<{
      id: string;
      kind: "ssh_private_key";
      hasValue: boolean;
    }>;
    passphrase: Readonly<{
      id: string;
      kind: "ssh_key_passphrase";
      hasValue: boolean;
    }> | null;
  }>;
export type KeyIdentityCatalogue = Readonly<{
  version: 1;
  token: string;
  revision: string;
  vaultPresent: boolean;
  identities: readonly KeyIdentity[];
}>;
export type KeyIdentityChange = Readonly<
  | ({ action: "create"; withPassphrase: boolean } & IdentityFields)
  | ({ action: "update"; id: string } & IdentityFields)
  | { action: "remove"; id: string }
  | { action: "remove_many"; ids: readonly string[] }
>;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
function bad(): never {
  throw Error("key-identities.invalid");
}
function record(
  input: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!input || typeof input !== "object" || Array.isArray(input)) return bad();
  const value = input as Record<string, unknown>;
  if (
    Object.keys(value).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(value, key))
  )
    return bad();
  return value;
}
export function validateKeyIdentityChange(
  input: unknown,
): Record<string, unknown> {
  if (!input || typeof input !== "object") return bad();
  if ((input as Record<string, unknown>).action === "create") {
    const row = record(input, [
      "action",
      "label",
      "username",
      "withPassphrase",
    ]);
    if (typeof row.withPassphrase !== "boolean") return bad();
    return {
      ...validateIdentityChange({
        action: "create",
        label: row.label,
        username: row.username,
      }),
      withPassphrase: row.withPassphrase,
    };
  }
  return validateIdentityChange(input);
}
function secret<K extends "ssh_private_key" | "ssh_key_passphrase">(
  input: unknown,
  kind: K,
): Readonly<{ id: string; kind: K; hasValue: boolean }> {
  const row = record(input, ["id", "kind", "hasValue"]);
  if (
    typeof row.id !== "string" ||
    !uuid.test(row.id) ||
    /^0{8}-0{4}-0{4}-0{4}-0{12}$/.test(row.id) ||
    row.kind !== kind ||
    typeof row.hasValue !== "boolean"
  )
    return bad();
  return Object.freeze({ id: row.id, kind, hasValue: row.hasValue });
}
export function decodeKeyIdentityCatalogue(
  input: unknown,
): KeyIdentityCatalogue {
  const value = record(input, [
    "version",
    "token",
    "revision",
    "vaultPresent",
    "identities",
  ]);
  if (
    value.version !== 1 ||
    typeof value.token !== "string" ||
    !uuid4.test(value.token) ||
    typeof value.revision !== "string" ||
    !/^(0|[1-9][0-9]{0,18})$/.test(value.revision) ||
    BigInt(value.revision) > 9223372036854775807n ||
    typeof value.vaultPresent !== "boolean" ||
    !Array.isArray(value.identities) ||
    value.identities.length > 256 ||
    (!value.vaultPresent && value.identities.length > 0)
  )
    return bad();
  const ids = new Set<string>();
  const identities = value.identities.map((input) => {
    const row = record(input, [
      "id",
      "label",
      "username",
      "secret",
      "passphrase",
    ]);
    if (typeof row.id !== "string" || !uuid7.test(row.id) || ids.has(row.id))
      return bad();
    ids.add(row.id);
    const fields = validateIdentityChange({
      action: "create",
      label: row.label,
      username: row.username,
    });
    const key = secret(row.secret, "ssh_private_key"),
      passphrase =
        row.passphrase === null
          ? null
          : secret(row.passphrase, "ssh_key_passphrase");
    if (passphrase?.id === key.id) return bad();
    return Object.freeze({
      id: row.id,
      label: fields.label as string,
      username: fields.username as string,
      secret: key,
      passphrase,
    });
  });
  return Object.freeze({
    version: 1,
    token: value.token,
    revision: value.revision,
    vaultPresent: value.vaultPresent,
    identities: Object.freeze(identities),
  });
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: IdentityRpc,
): Promise<KeyIdentityCatalogue> {
  if (signal.aborted) throw Error("key-identities.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined,
    abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("key-identities.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("key-identities.timeout")), 5000);
  });
  try {
    const raw = await Promise.race([rpc(command, args), stopped]);
    if (signal.aborted) throw Error("key-identities.cancelled");
    return decodeKeyIdentityCatalogue(raw);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readKeyIdentities(
  nonce: string,
  signal: AbortSignal,
  rpc: IdentityRpc = invoke,
): Promise<KeyIdentityCatalogue> {
  if (!uuid4.test(nonce)) return bad();
  return request("workspace_key_identities", { nonce }, signal, rpc);
}
export function saveKeyIdentity(
  nonce: string,
  token: string,
  change: KeyIdentityChange,
  signal: AbortSignal,
  rpc: IdentityRpc = invoke,
): Promise<KeyIdentityCatalogue> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) return bad();
  return request(
    "workspace_key_identity_save",
    { nonce, token, change: validateKeyIdentityChange(change) },
    signal,
    rpc,
  );
}
