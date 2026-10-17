import { invoke } from "@tauri-apps/api/core";
import {
  type IdentityFields,
  type IdentityRpc,
  validateIdentityChange,
} from "./workspace-identities";
export type AgentKey = Readonly<{ algorithm: string; fingerprint: string }>;
export type AgentIdentity = IdentityFields &
  AgentKey &
  Readonly<{ id: string }>;
export type AgentCatalogue = Readonly<{
  version: 1;
  token: string;
  revision: string;
  identities: readonly AgentIdentity[];
  keys: readonly AgentKey[];
}>;
export type AgentChange = Readonly<
  | ({ action: "create"; key: number } & IdentityFields)
  | ({ action: "update"; id: string } & IdentityFields)
  | { action: "remove"; id: string }
>;
const v4 =
    /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/,
  v7 = /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const algorithms = new Set([
  "ssh-ed25519",
  "ssh-rsa",
  "ecdsa-sha2-nistp256",
  "ecdsa-sha2-nistp384",
  "ecdsa-sha2-nistp521",
  "sk-ssh-ed25519@openssh.com",
  "sk-ecdsa-sha2-nistp256@openssh.com",
]);
function bad(): never {
  throw Error("agent-identities.invalid");
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
function key(r: Record<string, unknown>): AgentKey {
  if (
    typeof r.algorithm !== "string" ||
    !algorithms.has(r.algorithm) ||
    typeof r.fingerprint !== "string" ||
    !/^SHA256:[A-Za-z0-9+/]{42}[AEIMQUYcgkosw048]$/.test(r.fingerprint)
  )
    return bad();
  return Object.freeze({ algorithm: r.algorithm, fingerprint: r.fingerprint });
}
export function decodeAgentCatalogue(input: unknown): AgentCatalogue {
  const r = record(input, [
    "version",
    "token",
    "revision",
    "identities",
    "keys",
  ]);
  if (
    r.version !== 1 ||
    typeof r.token !== "string" ||
    !v4.test(r.token) ||
    typeof r.revision !== "string" ||
    !/^(0|[1-9][0-9]{0,18})$/.test(r.revision) ||
    BigInt(r.revision) > 9223372036854775807n ||
    !Array.isArray(r.identities) ||
    r.identities.length > 256 ||
    !Array.isArray(r.keys) ||
    r.keys.length > 128
  )
    return bad();
  const ids = new Set<string>();
  const identities = r.identities.map((v) => {
    const i = record(v, [
      "id",
      "label",
      "username",
      "algorithm",
      "fingerprint",
    ]);
    if (typeof i.id !== "string" || !v7.test(i.id) || ids.has(i.id))
      return bad();
    ids.add(i.id);
    const fields = validateIdentityChange({
      action: "create",
      label: i.label,
      username: i.username,
    });
    return Object.freeze({
      id: i.id,
      label: fields.label as string,
      username: fields.username as string,
      ...key(i),
    });
  });
  const fingerprints = new Set<string>();
  const keys = r.keys.map((v) => {
    const k = key(record(v, ["algorithm", "fingerprint"])),
      id = `${k.algorithm}:${k.fingerprint}`;
    if (fingerprints.has(id)) return bad();
    fingerprints.add(id);
    return k;
  });
  return Object.freeze({
    version: 1,
    token: r.token,
    revision: r.revision,
    identities: Object.freeze(identities),
    keys: Object.freeze(keys),
  });
}
export async function cancelAgentCatalogue(
  nonce: string,
  token: string,
  rpc: IdentityRpc = invoke,
): Promise<void> {
  if (!v4.test(token)) return;
  await rpc("workspace_agent_identity_cancel", { nonce, token }).catch(
    () => {},
  );
}
async function request(
  nonce: string,
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: IdentityRpc,
): Promise<AgentCatalogue> {
  if (signal.aborted) throw Error("agent-identities.cancelled");
  let stopped = false,
    timer: ReturnType<typeof setTimeout> | undefined,
    abort = () => {};
  const cancelled = new Promise<never>((_, reject) => {
    abort = () => {
      stopped = true;
      if (typeof args.token === "string")
        void cancelAgentCatalogue(nonce, args.token, rpc);
      reject(Error("agent-identities.cancelled"));
    };
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(abort, 5000);
  });
  const pending = rpc(command, { nonce, ...args }).then((raw) => {
    const result = decodeAgentCatalogue(raw);
    if (stopped || signal.aborted) {
      void cancelAgentCatalogue(nonce, result.token, rpc);
      throw Error("agent-identities.cancelled");
    }
    return result;
  });
  try {
    return await Promise.race([pending, cancelled]);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readAgentCatalogue(
  nonce: string,
  signal: AbortSignal,
  discover = false,
  rpc: IdentityRpc = invoke,
): Promise<AgentCatalogue> {
  return request(
    nonce,
    "workspace_agent_identities",
    { discover },
    signal,
    rpc,
  );
}
export function saveAgentCatalogue(
  nonce: string,
  catalogue: AgentCatalogue,
  input: AgentChange,
  signal: AbortSignal,
  rpc: IdentityRpc = invoke,
): Promise<AgentCatalogue> {
  if (!v4.test(catalogue.token))
    return Promise.reject(Error("agent-identities.invalid"));
  let change: Record<string, unknown>;
  if (input.action === "create") {
    record(input, ["action", "key", "label", "username"]);
    if (!Number.isSafeInteger(input.key) || !catalogue.keys[input.key])
      return Promise.reject(Error("agent-identities.invalid"));
    change = {
      ...validateIdentityChange({
        action: "create",
        label: input.label,
        username: input.username,
      }),
      key: input.key,
    };
  } else {
    change = validateIdentityChange(input);
    if (
      typeof change.id !== "string" ||
      !catalogue.identities.some((i) => i.id === change.id)
    )
      return Promise.reject(Error("agent-identities.invalid"));
  }
  return request(
    nonce,
    "workspace_agent_identity_save",
    { token: catalogue.token, change },
    signal,
    rpc,
  );
}
