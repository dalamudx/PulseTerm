import { describe, expect, it, vi } from "vitest";
import {
  decodeOrganization,
  filterOrganization,
  groupPath,
  requestOrganization,
} from "./workspace-organization";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const id = (n: number) =>
  `019a9c6d-7000-7000-8000-${n.toString(16).padStart(12, "0")}`;
const metadata = (n: number) => ({
  id: id(n),
  createdAt: 1000,
  updatedAt: 1000,
  hlc: "1000:0:01010101-0101-4101-8101-010101010101",
  deleted: false,
});
const fixture = () => ({
  token: "02020202-0202-4202-8202-020202020202",
  data: {
    groups: [
      { metadata: metadata(1), name: "中国", parent: null },
      { metadata: metadata(2), name: "开发", parent: id(1) },
    ],
    tags: [{ metadata: metadata(3), name: "生产", parent: null }],
    items: [
      {
        kind: "host",
        id: id(4),
        label: "Shell",
        description: "user@fixture.invalid:22",
      },
      { kind: "key_identity", id: id(5), label: "Key", description: "user" },
    ],
    memberships: [
      {
        metadata: metadata(6),
        kind: "host",
        item: id(4),
        group: id(2),
        tags: [id(3)],
        favorite: true,
      },
    ],
  },
});
it("same-source machine projection preserves entrance identities and rejects dangling or cross-kind relations", () => {
  const hostGraph = {
    version: 1,
    migrated: true,
    machines: [{ id: id(7), label: "Machine" }],
    entrances: [{ id: id(4), machineId: id(7) }],
  };
  const raw = { ...fixture(), data: { ...fixture().data, hostGraph } };
  const decoded = decodeOrganization(raw);
  expect(decoded.hostGraph?.entrances[0]?.id).toBe(id(4));
  expect(Object.isFrozen(decoded.hostGraph)).toBe(true);
  for (const bad of [
    { ...hostGraph, entrances: [{ id: id(5), machineId: id(7) }] },
    { ...hostGraph, entrances: [{ id: id(4), machineId: id(8) }] },
    { ...hostGraph, migrated: false },
    { ...hostGraph, entrances: [] },
  ])
    expect(() =>
      decodeOrganization({ ...raw, data: { ...raw.data, hostGraph: bad } }),
    ).toThrow("organization.invalid");
});
describe("organization metadata boundaries", () => {
  it("finds literal group paths and tag/favorite filters across saved directories", () => {
    const value = decodeOrganization(fixture());
    expect(groupPath(value.groups, id(2))).toBe("中国 / 开发");
    expect(
      filterOrganization(value, "生产", id(2), id(3), true).map((i) => i.id),
    ).toEqual([id(4)]);
    expect(
      filterOrganization(value, "", "ungrouped", "", false).map((i) => i.id),
    ).toEqual([id(5)]);
    expect(filterOrganization(value, "<script>", "", "", false)).toEqual([]);
  });
  it("rejects cycles, unknown fields, duplicate targets and dangling tags before drawing", () => {
    const cyclic = fixture();
    const [firstGroup] = cyclic.data.groups;
    if (!firstGroup) throw Error("fixture.group");
    firstGroup.parent = id(2) as unknown as null;
    expect(() => decodeOrganization(cyclic)).toThrow("organization.invalid");
    const duplicate = fixture();
    const [firstItem] = duplicate.data.items;
    if (!firstItem) throw Error("fixture.item");
    duplicate.data.items.push(firstItem);
    expect(() => decodeOrganization(duplicate)).toThrow();
    const missing = fixture();
    const [firstMember] = missing.data.memberships;
    if (!firstMember) throw Error("fixture.member");
    firstMember.tags = [id(99)];
    expect(() => decodeOrganization(missing)).toThrow();
    expect(() =>
      decodeOrganization({ ...fixture(), authority: true }),
    ).toThrow();
  });
  it("cancels before IPC and discards an already-running read after page revocation", async () => {
    const { invoke } = await import("@tauri-apps/api/core");
    const pre = new AbortController();
    pre.abort();
    await expect(requestOrganization("page", pre.signal)).rejects.toThrow(
      "organization.cancelled",
    );
    expect(invoke).not.toHaveBeenCalled();
    let resolve: ((value: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    const page = new AbortController();
    const result = requestOrganization("page", page.signal);
    page.abort();
    resolve?.(fixture());
    await expect(result).rejects.toThrow("organization.cancelled");
  });
});
