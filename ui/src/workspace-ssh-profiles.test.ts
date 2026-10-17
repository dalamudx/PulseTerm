import { expect, it, vi } from "vitest";
import {
  decodeSshProfiles,
  readSshProfiles,
  saveSshProfile,
  validateProfileChange,
} from "./workspace-ssh-profiles";

const token = "11111111-1111-4111-8111-111111111111";
const id = (n: number) =>
  `${String(n).padStart(8, "0")}-1111-7111-8111-111111111111`;
const payload = () => ({
  token,
  data: {
    version: 1,
    revision: "2",
    migrated: true,
    machines: [{ id: id(1), label: "Machine" }],
    entrances: [
      {
        id: id(2),
        label: "Entrance",
        host: "fixture.invalid",
        port: 22,
        username: "u",
        machineId: id(1),
      },
    ],
    profiles: [
      {
        id: id(3),
        endpointId: id(2),
        label: "Profile",
        username: "u",
        identityId: id(4),
      },
    ],
    identities: [
      {
        id: id(4),
        kind: "password",
        label: "Shared login",
        username: "u",
        available: true,
      },
      {
        id: id(5),
        kind: "key",
        label: "Shared key",
        username: "u",
        available: true,
      },
    ],
  },
});
it("retains independent profile/entrance/machine and both identity kinds without use material", () => {
  const view = decodeSshProfiles(payload());
  expect(view.profiles[0]?.label).toBe("Profile");
  expect(view.hostView.hosts[0]?.label).toBe("Entrance");
  expect(view.identities.map((i) => i.kind)).toEqual(["password", "key"]);
  const deleted = payload();
  deleted.data.identities = [];
  expect(decodeSshProfiles(deleted).profiles[0]?.identityId).toBe(id(4));
});
it("rejects unknown credential fields, cross-namespace IDs, source mismatch and kind ambiguity", () => {
  for (const mutate of [
    (raw: ReturnType<typeof payload>) =>
      Object.assign(raw.data.identities[0]!, { plaintext: "forbidden" }),
    (raw: ReturnType<typeof payload>) => {
      raw.data.profiles[0]!.endpointId = id(1);
    },
    (raw: ReturnType<typeof payload>) => {
      raw.data.profiles[0]!.id = id(2);
    },
    (raw: ReturnType<typeof payload>) => {
      raw.data.profiles[0]!.username = "other";
    },
    (raw: ReturnType<typeof payload>) => {
      raw.data.identities[1]!.id = id(4);
    },
  ]) {
    const raw = payload();
    mutate(raw);
    expect(() => decodeSshProfiles(raw)).toThrow();
  }
  const rpc = vi.fn(async () => payload());
  expect(() =>
    saveSshProfile(
      token,
      token,
      { id: id(3), label: "Profile", username: "u", identityId: "not-an-id" },
      new AbortController().signal,
      rpc,
    ),
  ).toThrow();
  expect(rpc).not.toHaveBeenCalled();
});
it("abort discards late projection and never retries a save", async () => {
  let resolve!: (raw: unknown) => void;
  const rpc = vi.fn(
      () =>
        new Promise<unknown>((done) => {
          resolve = done;
        }),
    ),
    life = new AbortController();
  const pending = readSshProfiles(token, life.signal, rpc);
  life.abort();
  await expect(pending).rejects.toThrow("cancelled");
  resolve(payload());
  expect(rpc).toHaveBeenCalledTimes(1);
});
function routedPayload() {
  const raw = payload();
  return {
    ...raw,
    data: {
      ...raw.data,
      version: 2,
      entrances: [
        ...raw.data.entrances,
        {
          ...raw.data.entrances[0],
          id: id(8),
          label: "Bastion",
          host: "jump.invalid",
        },
      ],
      profiles: [
        ...raw.data.profiles.map((p) => ({
          ...p,
          jumpProfileId: id(7) as string | null,
        })),
        {
          id: id(7),
          endpointId: id(8),
          label: "Jump",
          username: "u",
          identityId: null,
          jumpProfileId: null as string | null,
        },
      ],
    },
  };
}
it("v2 accepts exact jump references and rejects cycles, missing or cross-kind targets and version confusion", () => {
  const raw = routedPayload();
  expect(decodeSshProfiles(raw).profiles[0]?.jumpProfileId).toBe(id(7));
  expect(decodeSshProfiles(payload()).profiles[0]?.jumpProfileId).toBeNull();
  for (const reference of [id(1), id(4), id(99), id(3)]) {
    const bad = routedPayload();
    const first = bad.data.profiles[0];
    if (!first) throw Error("fixture.first");
    first.jumpProfileId = reference;
    expect(() => decodeSshProfiles(bad)).toThrow();
  }
  const cycle = routedPayload(),
    last = cycle.data.profiles[1];
  if (!last) throw Error("fixture.last");
  last.jumpProfileId = id(3);
  expect(() => decodeSshProfiles(cycle)).toThrow();
  for (const mutate of [
    (row: Record<string, unknown>) => {
      delete row.jumpProfileId;
    },
    (row: Record<string, unknown>) => {
      row.jumpProfileId = {};
    },
    (row: Record<string, unknown>) => {
      row.proxyCommand = "forbidden";
    },
  ]) {
    const bad = routedPayload(),
      first = bad.data.profiles[0];
    if (!first) throw Error("fixture.first");
    mutate(first);
    expect(() => decodeSshProfiles(bad)).toThrow();
  }
  const bad = routedPayload();
  bad.data.version = 1;
  expect(() => decodeSshProfiles(bad)).toThrow();
});
it("explicit v2 updates or clears via atomically while v1 omits route mutation", async () => {
  const base = { id: id(3), label: "Profile", username: "u", identityId: null };
  expect(validateProfileChange(base)).toEqual(base);
  for (const via of [null, id(7)]) {
    const change = { ...base, version: 2, via },
      rpc = vi.fn(async () => routedPayload());
    await saveSshProfile(
      token,
      token,
      validateProfileChange(change),
      new AbortController().signal,
      rpc,
    );
    expect(rpc).toHaveBeenCalledTimes(1);
    expect(rpc.mock.calls[0]).toEqual([
      "workspace_ssh_profile_save",
      { nonce: token, token, change },
    ]);
  }
  for (const raw of [
    { ...base, version: 2 },
    { ...base, via: null },
    { ...base, version: 3, via: null },
    { ...base, version: 2, via: id(3) },
  ])
    expect(() => validateProfileChange(raw)).toThrow();
});

it("v3 Profile projection accepts Agent references while v1/v2 reject the new kind", () => {
  const base = payload();
  const view = {
    ...base,
    data: {
      ...base.data,
      version: 3,
      profiles: base.data.profiles.map((p) => ({ ...p, jumpProfileId: null })),
      identities: [
        ...base.data.identities,
        {
          id: id(6),
          kind: "agent",
          label: "Agent",
          username: "u",
          available: true,
        },
      ],
    },
  };
  expect(decodeSshProfiles(view).identities.at(-1)?.kind).toBe("agent");
  for (const version of [1, 2]) {
    const data = {
      ...view.data,
      version,
      profiles: version === 1 ? base.data.profiles : view.data.profiles,
    };
    expect(() => decodeSshProfiles({ ...view, data })).toThrow();
  }
});
