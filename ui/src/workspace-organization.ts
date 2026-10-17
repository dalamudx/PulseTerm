import { invoke } from "@tauri-apps/api/core";
import { rankMetadata } from "./workspace-metadata-search";
export type OrganizationKind = "host" | "password_identity" | "key_identity";
type Metadata = Readonly<{
  id: string;
  createdAt: number;
  updatedAt: number;
  hlc: string;
  deleted: boolean;
}>;
export type OrganizationName = Readonly<{
  metadata: Metadata;
  name: string;
  parent: string | null;
}>;
export type OrganizationItem = Readonly<{
  kind: OrganizationKind;
  id: string;
  label: string;
  description: string;
}>;
export type OrganizationMembership = Readonly<{
  metadata: Metadata;
  kind: OrganizationKind;
  item: string;
  group: string | null;
  tags: readonly string[];
  favorite: boolean;
}>;
export type OrganizationHostGraph = Readonly<{
  version: 1;
  migrated: boolean;
  machines: readonly Readonly<{ id: string; label: string }>[];
  entrances: readonly Readonly<{ id: string; machineId: string }>[];
}>;
export type Organization = Readonly<{
  token: string;
  groups: readonly OrganizationName[];
  tags: readonly OrganizationName[];
  items: readonly OrganizationItem[];
  memberships: readonly OrganizationMembership[];
  hostGraph?: OrganizationHostGraph;
}>;
export type OrganizationChange =
  | { action: "create_group"; name: string; parent: string | null }
  | { action: "update_group"; id: string; name: string; parent: string | null }
  | { action: "remove_group"; id: string }
  | {
      action: "assign";
      kind: OrganizationKind;
      item: string;
      group: string | null;
      tags: readonly string[];
      favorite: boolean;
    };
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const hidden =
  /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u;
function invalid(): never {
  throw Error("organization.invalid");
}
function record(
  raw: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (
    !raw ||
    typeof raw !== "object" ||
    Array.isArray(raw) ||
    Object.keys(raw).length !== keys.length ||
    keys.some((k) => !Object.hasOwn(raw, k))
  )
    return invalid();
  return raw as Record<string, unknown>;
}
function text(raw: unknown, max: number, empty = false): string {
  if (
    typeof raw !== "string" ||
    (!empty && !raw) ||
    new TextEncoder().encode(raw).length > max ||
    hidden.test(raw)
  )
    return invalid();
  return raw;
}
function identity(raw: unknown): string {
  if (typeof raw !== "string" || !uuid7.test(raw)) return invalid();
  return raw;
}
function reference(raw: unknown): string | null {
  return raw === null ? null : identity(raw);
}
function kind(raw: unknown): OrganizationKind {
  if (raw !== "host" && raw !== "password_identity" && raw !== "key_identity")
    return invalid();
  return raw;
}
function array<T>(
  raw: unknown,
  max: number,
  parse: (value: unknown) => T,
): T[] {
  if (!Array.isArray(raw) || raw.length > max) return invalid();
  return raw.map(parse);
}
function metadata(raw: unknown): Metadata {
  const r = record(raw, ["id", "createdAt", "updatedAt", "hlc", "deleted"]);
  if (
    !Number.isSafeInteger(r.createdAt) ||
    (r.createdAt as number) < 1 ||
    !Number.isSafeInteger(r.updatedAt) ||
    (r.updatedAt as number) < (r.createdAt as number) ||
    typeof r.deleted !== "boolean"
  )
    return invalid();
  return {
    id: identity(r.id),
    createdAt: r.createdAt as number,
    updatedAt: r.updatedAt as number,
    hlc: text(r.hlc, 67),
    deleted: r.deleted,
  };
}
function named(raw: unknown): OrganizationName {
  const r = record(raw, ["metadata", "name", "parent"]);
  const name = text(r.name, 128);
  if (name.trim() !== name) return invalid();
  return { metadata: metadata(r.metadata), name, parent: reference(r.parent) };
}
export function groupPath(
  groups: readonly OrganizationName[],
  id: string,
): string {
  const path: string[] = [];
  let current: string | null = id;
  const visited = new Set<string>();
  while (current) {
    if (visited.has(current) || path.length >= 8) return invalid();
    visited.add(current);
    const group = groups.find(
      (g) => g.metadata.id === current && !g.metadata.deleted,
    );
    if (!group) return invalid();
    path.unshift(group.name);
    current = group.parent;
  }
  return path.join(" / ");
}
export function decodeOrganization(raw: unknown): Organization {
  const outer = record(raw, ["token", "data"]);
  if (typeof outer.token !== "string" || !uuid4.test(outer.token))
    return invalid();
  const hasGraph =
    !!outer.data &&
    typeof outer.data === "object" &&
    Object.hasOwn(outer.data, "hostGraph");
  const d = record(outer.data, [
    "groups",
    "tags",
    "items",
    "memberships",
    ...(hasGraph ? ["hostGraph"] : []),
  ]);
  const groups = array(d.groups, 256, named),
    tags = array(d.tags, 256, named);
  const items = array(d.items, 768, (raw) => {
    const r = record(raw, ["kind", "id", "label", "description"]);
    return {
      kind: kind(r.kind),
      id: identity(r.id),
      label: text(r.label, 256),
      description: text(r.description, 1024),
    };
  });
  const memberships = array(d.memberships, 1024, (raw) => {
    const r = record(raw, [
      "metadata",
      "kind",
      "item",
      "group",
      "tags",
      "favorite",
    ]);
    if (typeof r.favorite !== "boolean") return invalid();
    return {
      metadata: metadata(r.metadata),
      kind: kind(r.kind),
      item: identity(r.item),
      group: reference(r.group),
      tags: array(r.tags, 16, identity),
      favorite: r.favorite,
    };
  });
  const ids = new Set<string>();
  for (const row of [...groups, ...tags, ...memberships]) {
    if (ids.has(row.metadata.id)) return invalid();
    ids.add(row.metadata.id);
  }
  for (const group of groups)
    if (!group.metadata.deleted) groupPath(groups, group.metadata.id);
  for (const tag of tags) if (tag.parent !== null) return invalid();
  const targets = new Set<string>();
  for (const item of items) {
    const key = `${item.kind}:${item.id}`;
    if (targets.has(key)) return invalid();
    targets.add(key);
  }
  const refs = new Set<string>();
  for (const row of memberships) {
    const key = `${row.kind}:${row.item}`;
    if (refs.has(key)) return invalid();
    refs.add(key);
    if (
      row.group &&
      !groups.some((g) => g.metadata.id === row.group && !g.metadata.deleted)
    )
      return invalid();
    if (
      new Set(row.tags).size !== row.tags.length ||
      row.tags.some(
        (id) => !tags.some((t) => t.metadata.id === id && !t.metadata.deleted),
      )
    )
      return invalid();
  }
  let hostGraph: OrganizationHostGraph | undefined;
  if (hasGraph) {
    const g = record(d.hostGraph, [
      "version",
      "migrated",
      "machines",
      "entrances",
    ]);
    if (g.version !== 1 || typeof g.migrated !== "boolean") return invalid();
    const machines = array(g.machines, 256, (raw) => {
      const m = record(raw, ["id", "label"]);
      return Object.freeze({ id: identity(m.id), label: text(m.label, 256) });
    });
    const machineIds = new Set(machines.map((m) => m.id));
    if (machineIds.size !== machines.length) return invalid();
    const entrances = array(g.entrances, 256, (raw) => {
      const e = record(raw, ["id", "machineId"]);
      return Object.freeze({
        id: identity(e.id),
        machineId: identity(e.machineId),
      });
    });
    const entranceIds = new Set(entrances.map((e) => e.id));
    const hostIds = new Set(
      items.filter((i) => i.kind === "host").map((i) => i.id),
    );
    if (
      entranceIds.size !== entrances.length ||
      entrances.some(
        (e) =>
          !hostIds.has(e.id) ||
          !machineIds.has(e.machineId) ||
          machineIds.has(e.id),
      ) ||
      (!g.migrated && (machines.length || entrances.length)) ||
      (g.migrated && entranceIds.size !== hostIds.size) ||
      machines.some((m) => !entrances.some((e) => e.machineId === m.id))
    )
      return invalid();
    hostGraph = Object.freeze({
      version: 1,
      migrated: g.migrated,
      machines: Object.freeze(machines),
      entrances: Object.freeze(entrances),
    });
  }
  return {
    token: outer.token,
    groups,
    tags,
    items,
    memberships,
    ...(hostGraph ? { hostGraph } : {}),
  };
}
export function filterOrganization(
  value: Organization,
  query: string,
  group: string,
  tag: string,
  favorites: boolean,
): readonly OrganizationItem[] {
  const memberships = new Map(
    value.memberships
      .filter((m) => !m.metadata.deleted)
      .map((m) => [`${m.kind}:${m.item}`, m]),
  );
  const tags = new Map(
    value.tags
      .filter((t) => !t.metadata.deleted)
      .map((t) => [t.metadata.id, t.name]),
  );
  const paths = new Map(
    value.groups
      .filter((g) => !g.metadata.deleted)
      .map((g) => [g.metadata.id, groupPath(value.groups, g.metadata.id)]),
  );
  const membership = (item: OrganizationItem) =>
    memberships.get(`${item.kind}:${item.id}`);
  const visible = value.items.filter((item) => {
    const m = membership(item);
    return (
      (!group || (group === "ungrouped" ? !m?.group : m?.group === group)) &&
      (!tag || m?.tags.includes(tag)) &&
      (!favorites || m?.favorite)
    );
  });
  return rankMetadata(visible, query, (item) => {
    const m = membership(item);
    return [
      item.label,
      item.description,
      ...(m?.tags ?? []).map((id) => tags.get(id) ?? ""),
      m?.group ? (paths.get(m.group) ?? "") : "",
    ];
  });
}
export async function requestOrganization(
  nonce: string,
  signal: AbortSignal,
  change?: { token: string; change: OrganizationChange },
): Promise<Organization> {
  if (signal.aborted) throw Error("organization.cancelled");
  const raw = await invoke(
    change ? "workspace_organization_save" : "workspace_organization",
    { nonce, ...change },
  );
  if (signal.aborted) throw Error("organization.cancelled");
  return decodeOrganization(raw);
}
