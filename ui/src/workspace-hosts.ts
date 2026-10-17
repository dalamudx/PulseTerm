import { invoke } from "@tauri-apps/api/core";
import { validateCatalogueIds } from "./workspace-catalogue-batch";
export type HostFields = Readonly<{
  label: string;
  host: string;
  port: number;
  username: string;
}>;
export type HostProfile = Readonly<{
  id: string;
  label: string;
  identityId: string | null;
  via?: string | null;
}>;
export type Host = HostFields &
  Readonly<{ id: string; machineId?: string; profile?: HostProfile }>;
export type Machine = Readonly<{ id: string; label: string }>;
export type HostCatalogue = Readonly<{
  version: 1 | 2 | 3 | 4;
  machines?: readonly Machine[];
  migrated?: boolean;
  token: string;
  revision: string;
  hosts: readonly Host[];
}>;
export type HostChange = Readonly<
  | ({ action: "create" } & HostFields)
  | ({ action: "attach"; machineId: string } & HostFields)
  | { action: "migrate_catalogue" }
  | { action: "rename_machine"; id: string; label: string }
  | ({ action: "update"; id: string } & HostFields)
  | { action: "remove"; id: string }
  | { action: "remove_many"; ids: readonly string[] }
>;
export type HostRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const hidden =
  /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u;
function bad(): never {
  throw Error("hosts.invalid");
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
    input.length === 0 ||
    input.length > max ||
    new TextEncoder().encode(input).length > max ||
    hidden.test(input)
  )
    return bad();
  return input;
}
function fields(r: Record<string, unknown>): HostFields {
  const label = text(r.label, 256),
    host = text(r.host, 255),
    username = text(r.username, 255);
  if (
    label.trim() !== label ||
    /\s/u.test(username) ||
    username.startsWith("-") ||
    !/^[a-zA-Z0-9.:-]+$/.test(host) ||
    host.startsWith("-") ||
    typeof r.port !== "number" ||
    !Number.isInteger(r.port) ||
    r.port < 1 ||
    r.port > 65535
  )
    return bad();
  return Object.freeze({ label, host, port: r.port, username });
}
export function validateHostChange(input: unknown): HostChange {
  if (!input || typeof input !== "object") return bad();
  const action = (input as Record<string, unknown>).action;
  if (action === "migrate_catalogue") {
    record(input, ["action"]);
    return Object.freeze({ action });
  }
  if (action === "rename_machine") {
    const r = record(input, ["action", "id", "label"]);
    if (typeof r.id !== "string" || !uuid7.test(r.id)) return bad();
    const label = text(r.label, 256);
    if (label.trim() !== label) return bad();
    return Object.freeze({ action, id: r.id, label });
  }
  if (action === "attach") {
    const r = record(input, [
      "action",
      "machineId",
      "label",
      "host",
      "port",
      "username",
    ]);
    if (typeof r.machineId !== "string" || !uuid7.test(r.machineId))
      return bad();
    return Object.freeze({ action, machineId: r.machineId, ...fields(r) });
  }
  if (action === "remove_many") {
    const r = record(input, ["action", "ids"]);
    return Object.freeze({ action, ids: validateCatalogueIds(r.ids) });
  }
  if (action === "create") {
    const r = record(input, ["action", "label", "host", "port", "username"]);
    return Object.freeze({ action, ...fields(r) });
  }
  if (action === "update" || action === "remove") {
    const r = record(
      input,
      action === "update"
        ? ["action", "id", "label", "host", "port", "username"]
        : ["action", "id"],
    );
    if (typeof r.id !== "string" || !uuid7.test(r.id)) return bad();
    return action === "remove"
      ? Object.freeze({ action, id: r.id })
      : Object.freeze({ action, id: r.id, ...fields(r) });
  }
  return bad();
}
export function decodeHostCatalogue(input: unknown): HostCatalogue {
  const v4 =
    !!input &&
    typeof input === "object" &&
    (input as Record<string, unknown>).version === 4;
  const v3 =
    !!input &&
    typeof input === "object" &&
    ((input as Record<string, unknown>).version === 3 || v4);
  const v2 =
    !!input &&
    typeof input === "object" &&
    ((input as Record<string, unknown>).version === 2 || v3);
  const r = record(
    input,
    v2
      ? ["version", "token", "revision", "hosts", "machines", "migrated"]
      : ["version", "token", "revision", "hosts"],
  );
  if (
    (r.version !== 1 &&
      r.version !== 2 &&
      r.version !== 3 &&
      r.version !== 4) ||
    typeof r.token !== "string" ||
    !uuid4.test(r.token) ||
    typeof r.revision !== "string" ||
    !/^(0|[1-9][0-9]{0,19})$/.test(r.revision) ||
    BigInt(r.revision) > 18446744073709551615n ||
    !Array.isArray(r.hosts) ||
    r.hosts.length > 256
  )
    return bad();
  const machineIds = new Set<string>();
  const machines: Machine[] = [];
  if (v2) {
    if (
      typeof r.migrated !== "boolean" ||
      !Array.isArray(r.machines) ||
      r.machines.length > 256 ||
      (!r.migrated && r.machines.length)
    )
      return bad();
    for (const row of r.machines) {
      const m = record(row, ["id", "label"]);
      if (typeof m.id !== "string" || !uuid7.test(m.id) || machineIds.has(m.id))
        return bad();
      const label = text(m.label, 256);
      if (label.trim() !== label) return bad();
      machineIds.add(m.id);
      machines.push(Object.freeze({ id: m.id, label }));
    }
  }
  const ids = new Set<string>();
  const profileIds = new Set<string>();
  const hosts = r.hosts.map((input) => {
    const h = record(
      input,
      v2
        ? [
            "id",
            "label",
            "host",
            "port",
            "username",
            "machineId",
            ...(v3 ? ["profile"] : []),
          ]
        : ["id", "label", "host", "port", "username"],
    );
    if (
      typeof h.id !== "string" ||
      !uuid7.test(h.id) ||
      ids.has(h.id) ||
      machineIds.has(h.id)
    )
      return bad();
    ids.add(h.id);
    const value = fields(h);
    if (value.host !== value.host.toLowerCase()) return bad();
    if (v2) {
      if (r.migrated) {
        if (typeof h.machineId !== "string" || !machineIds.has(h.machineId))
          return bad();
      } else if (h.machineId !== null) return bad();
    }
    let profile: HostProfile | undefined;
    if (v3) {
      if (!r.migrated && h.profile !== null) return bad();
      if (r.migrated) {
        const p = record(
          h.profile,
          v4
            ? ["id", "label", "identityId", "via"]
            : ["id", "label", "identityId"],
        );
        if (
          typeof p.id !== "string" ||
          !uuid7.test(p.id) ||
          profileIds.has(p.id) ||
          (p.identityId !== null &&
            (typeof p.identityId !== "string" || !uuid7.test(p.identityId)))
        )
          return bad();
        if (
          v4 &&
          p.via !== null &&
          (typeof p.via !== "string" || !uuid7.test(p.via) || p.via === p.id)
        )
          return bad();
        const label = text(p.label, 256);
        if (label.trim() !== label) return bad();
        profileIds.add(p.id);
        profile = Object.freeze({
          id: p.id,
          label,
          identityId: p.identityId as string | null,
          ...(v4 ? { via: p.via as string | null } : {}),
        });
      }
    }
    return Object.freeze({
      id: h.id,
      ...(profile ? { profile } : {}),
      ...value,
      ...(v2 && typeof h.machineId === "string"
        ? { machineId: h.machineId }
        : {}),
    });
  });
  if (
    [...profileIds].some((id) => ids.has(id) || machineIds.has(id)) ||
    hosts.some(
      (h) =>
        h.profile?.via &&
        (!profileIds.has(h.profile.via) || h.profile.via === h.profile.id),
    ) ||
    hosts.some(
      (h) =>
        h.profile?.identityId &&
        (ids.has(h.profile.identityId) ||
          machineIds.has(h.profile.identityId) ||
          profileIds.has(h.profile.identityId)),
    )
  )
    return bad();
  return Object.freeze({
    version: r.version,
    token: r.token,
    revision: r.revision,
    hosts: Object.freeze(hosts),
    ...(v2
      ? { machines: Object.freeze(machines), migrated: r.migrated as boolean }
      : {}),
  });
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: HostRpc,
): Promise<HostCatalogue> {
  if (signal.aborted) throw Error("hosts.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("hosts.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("hosts.timeout")), 5000);
  });
  try {
    const raw = await Promise.race([rpc(command, args), stopped]);
    if (signal.aborted) throw Error("hosts.cancelled");
    return decodeHostCatalogue(raw);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readHosts(
  nonce: string,
  signal: AbortSignal,
  rpc: HostRpc = invoke,
): Promise<HostCatalogue> {
  if (!uuid4.test(nonce)) return bad();
  return request("workspace_hosts", { nonce }, signal, rpc);
}
export function saveHost(
  nonce: string,
  token: string,
  change: HostChange,
  signal: AbortSignal,
  rpc: HostRpc = invoke,
): Promise<HostCatalogue> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) return bad();
  return request(
    "workspace_host_save",
    { nonce, token, change: validateHostChange(change) },
    signal,
    rpc,
  );
}
