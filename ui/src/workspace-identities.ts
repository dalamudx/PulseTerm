import { invoke } from "@tauri-apps/api/core";
import { validateCatalogueIds } from "./workspace-catalogue-batch";
export type IdentityFields = Readonly<{ label: string; username: string }>;
export type Identity = IdentityFields &
  Readonly<{
    id: string;
    secret: Readonly<{ id: string; kind: "ssh_password"; hasValue: boolean }>;
  }>;
export type IdentityCatalogue = Readonly<{
  version: 1;
  token: string;
  revision: string;
  vaultPresent: boolean;
  identities: readonly Identity[];
}>;
export type IdentityChange = Readonly<
  | ({ action: "create" } & IdentityFields)
  | ({ action: "update"; id: string } & IdentityFields)
  | { action: "remove"; id: string }
  | { action: "remove_many"; ids: readonly string[] }
>;
export type IdentityRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const hidden =
  /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u;
function bad(): never {
  throw Error("identities.invalid");
}
function record(
  input: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!input || typeof input !== "object" || Array.isArray(input)) return bad();
  const r = input as Record<string, unknown>;
  if (
    Object.keys(r).length !== keys.length ||
    keys.some((k) => !Object.hasOwn(r, k))
  )
    return bad();
  return r;
}
function text(input: unknown, max: number): string {
  if (
    typeof input !== "string" ||
    !input.length ||
    input.length > max ||
    new TextEncoder().encode(input).length > max ||
    hidden.test(input)
  )
    return bad();
  return input;
}
function fields(r: Record<string, unknown>): IdentityFields {
  const label = text(r.label, 256),
    username = text(r.username, 255);
  if (
    label.trim() !== label ||
    /\s/u.test(username) ||
    username.startsWith("-")
  )
    return bad();
  return Object.freeze({ label, username });
}
export function validateIdentityChange(
  input: unknown,
): Record<string, unknown> {
  if (!input || typeof input !== "object") return bad();
  const action = (input as Record<string, unknown>).action;
  if (action === "remove_many") {
    const r = record(input, ["action", "ids"]);
    return { operation: action, ids: validateCatalogueIds(r.ids) };
  }
  if (action === "create") {
    const r = record(input, ["action", "label", "username"]);
    return { operation: "create", ...fields(r) };
  }
  if (action === "update" || action === "remove") {
    const r = record(
      input,
      action === "update"
        ? ["action", "id", "label", "username"]
        : ["action", "id"],
    );
    if (typeof r.id !== "string" || !uuid7.test(r.id)) return bad();
    return action === "update"
      ? { operation: action, id: r.id, ...fields(r) }
      : { operation: action, id: r.id };
  }
  return bad();
}
export function decodeIdentityCatalogue(input: unknown): IdentityCatalogue {
  const r = record(input, [
    "version",
    "token",
    "revision",
    "vaultPresent",
    "identities",
  ]);
  if (
    r.version !== 1 ||
    typeof r.token !== "string" ||
    !uuid4.test(r.token) ||
    typeof r.revision !== "string" ||
    !/^(0|[1-9][0-9]{0,18})$/.test(r.revision) ||
    BigInt(r.revision) > 9223372036854775807n ||
    typeof r.vaultPresent !== "boolean" ||
    !Array.isArray(r.identities) ||
    r.identities.length > 256 ||
    (!r.vaultPresent && r.identities.length > 0)
  )
    return bad();
  const ids = new Set<string>();
  const identities = r.identities.map((input) => {
    const row = record(input, ["id", "label", "username", "secret"]);
    if (typeof row.id !== "string" || !uuid7.test(row.id) || ids.has(row.id))
      return bad();
    ids.add(row.id);
    const secret = record(row.secret, ["id", "kind", "hasValue"]);
    if (
      typeof secret.id !== "string" ||
      !uuid.test(secret.id) ||
      /^0{8}-0{4}-0{4}-0{4}-0{12}$/.test(secret.id) ||
      secret.kind !== "ssh_password" ||
      typeof secret.hasValue !== "boolean"
    )
      return bad();
    return Object.freeze({
      id: row.id,
      ...fields(row),
      secret: Object.freeze({
        id: secret.id,
        kind: "ssh_password" as const,
        hasValue: secret.hasValue,
      }),
    });
  });
  return Object.freeze({
    version: 1,
    token: r.token,
    revision: r.revision,
    vaultPresent: r.vaultPresent,
    identities: Object.freeze(identities),
  });
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: IdentityRpc,
): Promise<IdentityCatalogue> {
  if (signal.aborted) throw Error("identities.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("identities.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("identities.timeout")), 5000);
  });
  try {
    const raw = await Promise.race([rpc(command, args), stopped]);
    if (signal.aborted) throw Error("identities.cancelled");
    return decodeIdentityCatalogue(raw);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readIdentities(
  nonce: string,
  signal: AbortSignal,
  rpc: IdentityRpc = invoke,
): Promise<IdentityCatalogue> {
  if (!uuid4.test(nonce)) return bad();
  return request("workspace_identities", { nonce }, signal, rpc);
}
export function saveIdentity(
  nonce: string,
  token: string,
  change: IdentityChange,
  signal: AbortSignal,
  rpc: IdentityRpc = invoke,
): Promise<IdentityCatalogue> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) return bad();
  return request(
    "workspace_identity_save",
    { nonce, token, change: validateIdentityChange(change) },
    signal,
    rpc,
  );
}
