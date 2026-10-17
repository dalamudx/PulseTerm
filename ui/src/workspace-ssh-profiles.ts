import { invoke } from "@tauri-apps/api/core";
import {
  decodeHostCatalogue,
  type HostCatalogue,
  validateHostChange,
} from "./workspace-hosts";
export type SshProfile = Readonly<{
  id: string;
  endpointId: string;
  label: string;
  username: string;
  identityId: string | null;
  jumpProfileId: string | null;
}>;
export type ProfileIdentity = Readonly<{
  id: string;
  kind: "password" | "key" | "agent";
  label: string;
  username: string;
  available: boolean;
}>;
export type ProfileCatalogue = Readonly<{
  token: string;
  revision: string;
  migrated: boolean;
  profiles: readonly SshProfile[];
  identities: readonly ProfileIdentity[];
  hostView: Pick<HostCatalogue, "hosts" | "machines" | "migrated">;
}>;
export type ProfileChange = Readonly<{
  version?: 2;
  via?: string | null;
  id: string;
  label: string;
  username: string;
  identityId: string | null;
}>;
export type ProfileRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
function invalid(): never {
  throw Error("ssh_profiles.invalid");
}
function record(
  raw: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return invalid();
  const row = raw as Record<string, unknown>;
  if (
    Object.keys(row).length !== keys.length ||
    keys.some((k) => !Object.hasOwn(row, k))
  )
    return invalid();
  return row;
}
function id(raw: unknown): string {
  if (typeof raw !== "string" || !uuid7.test(raw)) return invalid();
  return raw;
}
function fields(
  label: unknown,
  username: unknown,
): { label: string; username: string } {
  const value = validateHostChange({
    action: "create",
    label,
    host: "fixture.invalid",
    port: 22,
    username,
  } as Parameters<typeof validateHostChange>[0]);
  if (
    value.action !== "create" ||
    value.label !== label ||
    value.username !== username
  )
    return invalid();
  return { label: value.label, username: value.username };
}
export function validateProfileChange(raw: unknown): ProfileChange {
  const routed =
    !!raw && typeof raw === "object" && Object.hasOwn(raw, "version");
  const row = record(raw, [
    "id",
    "label",
    "username",
    "identityId",
    ...(routed ? ["version", "via"] : []),
  ]);
  if (routed && row.version !== 2) return invalid();
  const via = routed ? (row.via === null ? null : id(row.via)) : undefined;
  if (via === row.id) return invalid();
  return Object.freeze({
    id: id(row.id),
    ...fields(row.label, row.username),
    identityId: row.identityId === null ? null : id(row.identityId),
    ...(routed ? { version: 2 as const, via } : {}),
  });
}
export function decodeSshProfiles(raw: unknown): ProfileCatalogue {
  const outer = record(raw, ["token", "data"]);
  if (typeof outer.token !== "string" || !uuid4.test(outer.token))
    return invalid();
  const data = record(outer.data, [
    "version",
    "revision",
    "migrated",
    "machines",
    "entrances",
    "profiles",
    "identities",
  ]);
  if (
    (data.version !== 1 && data.version !== 2 && data.version !== 3) ||
    typeof data.migrated !== "boolean" ||
    !Array.isArray(data.profiles) ||
    data.profiles.length > 256 ||
    !Array.isArray(data.identities) ||
    data.identities.length > (data.version === 3 ? 768 : 512)
  )
    return invalid();
  const view = decodeHostCatalogue({
    version: 2,
    token: outer.token,
    revision: data.revision,
    migrated: data.migrated,
    machines: data.machines,
    hosts: data.entrances,
  });
  const ids = new Set([
    ...(view.machines ?? []).map((m) => m.id),
    ...view.hosts.map((h) => h.id),
  ]);
  const endpoints = new Set<string>();
  const profiles = data.profiles.map((raw) => {
    const row = record(raw, [
      "id",
      "endpointId",
      "label",
      "username",
      "identityId",
      ...(data.version !== 1 ? ["jumpProfileId"] : []),
    ]);
    const profileId = id(row.id),
      endpointId = id(row.endpointId);
    if (
      ids.has(profileId) ||
      endpoints.has(endpointId) ||
      !view.hosts.some(
        (h) => h.id === endpointId && h.username === row.username,
      )
    )
      return invalid();
    ids.add(profileId);
    endpoints.add(endpointId);
    return Object.freeze({
      id: profileId,
      endpointId,
      ...fields(row.label, row.username),
      identityId: row.identityId === null ? null : id(row.identityId),
      jumpProfileId:
        data.version === 1 || row.jumpProfileId === null
          ? null
          : id(row.jumpProfileId),
    });
  });
  if (
    (data.migrated && profiles.length !== view.hosts.length) ||
    (!data.migrated && profiles.length)
  )
    return invalid();
  const byId = new Map(profiles.map((p) => [p.id, p]));
  for (const profile of profiles) {
    let via = profile.jumpProfileId;
    const seen = new Set([profile.id]);
    let hops = 0;
    while (via !== null) {
      if (hops++ >= 8 || seen.has(via)) return invalid();
      const next = byId.get(via);
      if (!next) return invalid();
      seen.add(via);
      via = next.jumpProfileId;
    }
  }
  const identities = data.identities.map((raw) => {
    const row = record(raw, ["id", "kind", "label", "username", "available"]),
      identityId = id(row.id);
    if (
      ids.has(identityId) ||
      (row.kind !== "password" &&
        row.kind !== "key" &&
        !(data.version === 3 && row.kind === "agent")) ||
      typeof row.available !== "boolean"
    )
      return invalid();
    ids.add(identityId);
    return Object.freeze({
      id: identityId,
      kind: row.kind,
      ...fields(row.label, row.username),
      available: row.available,
    });
  });
  return Object.freeze({
    token: outer.token,
    revision: view.revision,
    migrated: data.migrated,
    profiles: Object.freeze(profiles),
    identities: Object.freeze(identities),
    hostView: Object.freeze({
      hosts: view.hosts,
      machines: view.machines,
      migrated: view.migrated,
    }),
  });
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: ProfileRpc,
): Promise<ProfileCatalogue> {
  if (signal.aborted) throw Error("ssh_profiles.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("ssh_profiles.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("ssh_profiles.timeout")), 5000);
  });
  try {
    const raw = await Promise.race([rpc(command, args), stopped]);
    if (signal.aborted) throw Error("ssh_profiles.cancelled");
    return decodeSshProfiles(raw);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readSshProfiles(
  nonce: string,
  signal: AbortSignal,
  rpc: ProfileRpc = invoke,
): Promise<ProfileCatalogue> {
  if (!uuid4.test(nonce)) return invalid();
  return request("workspace_ssh_profiles", { nonce }, signal, rpc);
}
export function saveSshProfile(
  nonce: string,
  token: string,
  change: ProfileChange,
  signal: AbortSignal,
  rpc: ProfileRpc = invoke,
): Promise<ProfileCatalogue> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) return invalid();
  return request(
    "workspace_ssh_profile_save",
    { nonce, token, change: validateProfileChange(change) },
    signal,
    rpc,
  );
}
