import { expect, it, vi } from "vitest";
import {
  decodeHostCatalogue,
  readHosts,
  saveHost,
  validateHostChange,
} from "./workspace-hosts";

const token = "11111111-1111-4111-8111-111111111111",
  id = "11111111-1111-7111-8111-111111111111";
const host = {
  id,
  label: "开发主机",
  host: "example.com",
  port: 22,
  username: "user",
};
const value = () => ({
  version: 1,
  token,
  revision: "18446744073709551615",
  hosts: [{ ...host }],
});
const machineId = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb";
function graphValue() {
  return {
    ...value(),
    version: 2,
    migrated: true,
    machines: [{ id: machineId, label: "机器" }],
    hosts: [{ ...host, machineId }],
  };
}
it("v2 rejects absent, foreign or cross-kind machine references instead of falling back", () => {
  expect(decodeHostCatalogue(graphValue()).hosts[0]?.machineId).toBe(machineId);
  for (const input of [
    { ...graphValue(), machines: [] },
    { ...graphValue(), migrated: false },
    { ...graphValue(), machines: [{ id, label: "collision" }] },
    { ...graphValue(), hosts: [{ ...host, machineId: null }] },
    {
      ...graphValue(),
      hosts: [{ ...host, machineId: "cccccccc-cccc-7ccc-8ccc-cccccccccccc" }],
    },
  ])
    expect(() => decodeHostCatalogue(input)).toThrow("hosts.invalid");
  expect(
    decodeHostCatalogue({
      ...graphValue(),
      migrated: false,
      machines: [],
      hosts: [{ ...host, machineId: null }],
    }).migrated,
  ).toBe(false);
});
it("migration, machine rename and attach are distinct bounded metadata intents", () => {
  expect(validateHostChange({ action: "migrate_catalogue" })).toEqual({
    action: "migrate_catalogue",
  });
  expect(() =>
    validateHostChange({ action: "attach", machineId, ...host, id: undefined }),
  ).toThrow("hosts.invalid");
  expect(
    validateHostChange({
      action: "attach",
      machineId,
      label: host.label,
      host: host.host,
      port: host.port,
      username: host.username,
    }).action,
  ).toBe("attach");
  expect(
    validateHostChange({
      action: "rename_machine",
      id: machineId,
      label: "Machine",
    }).action,
  ).toBe("rename_machine");
});
it("decodes exact u64 revisions and immutable bounded metadata", () => {
  const decoded = decodeHostCatalogue(value());
  expect(decoded.revision).toBe("18446744073709551615");
  expect(Object.isFrozen(decoded.hosts[0])).toBe(true);
  expect(Object.isFrozen(decoded.hosts)).toBe(true);
});
it("rejects invalid versions, extra secrets, overflow, duplicates and excessive rows", () => {
  for (const input of [
    { ...value(), version: 2 },
    { ...value(), password: "secret" },
    { ...value(), revision: "18446744073709551616" },
    { ...value(), revision: "01" },
    { ...value(), hosts: [host, host] },
    { ...value(), hosts: Array(257).fill(host) },
    { ...value(), hosts: [{ ...host, host: "EXAMPLE.COM" }] },
    { ...value(), hosts: [{ ...host, id: token }] },
  ])
    expect(() => decodeHostCatalogue(input)).toThrow("hosts.invalid");
});
it("rejects byte overflow/control/bidi and frontend paths/credentials", () => {
  for (const change of [
    { action: "create", ...host, id: undefined },
    {
      action: "create",
      label: "主".repeat(86),
      host: "host",
      port: 22,
      username: "user",
    },
    { action: "update", ...host, username: "-option" },
    { action: "update", ...host, label: "a\u202eb" },
    { action: "update", ...host, host: "[::1]" },
    { action: "update", ...host, port: 0 },
    { action: "update", ...host, path: "/tmp/db" },
    { action: "remove", id: token },
  ])
    expect(() => validateHostChange(change)).toThrow();
});
it("sends validated edit fields/opaque ticket only", async () => {
  const rpc = vi.fn(async () => value());
  await saveHost(
    token,
    token,
    { action: "update", ...host },
    new AbortController().signal,
    rpc,
  );
  expect(rpc).toHaveBeenCalledWith("workspace_host_save", {
    nonce: token,
    token,
    change: { action: "update", ...host },
  });
});
it("cancelled reads ignore late values; already cancelled requests never call RPC", async () => {
  let resolve!: (x: unknown) => void;
  const rpc = vi.fn(
    () =>
      new Promise<unknown>((r) => {
        resolve = r;
      }),
  );
  const life = new AbortController();
  const pending = readHosts(token, life.signal, rpc);
  life.abort();
  await expect(pending).rejects.toThrow("hosts.cancelled");
  resolve(value());
  await expect(readHosts(token, life.signal, rpc)).rejects.toThrow(
    "hosts.cancelled",
  );
  expect(rpc).toHaveBeenCalledTimes(1);
});
it("v3 carries independent profile defaults and rejects malformed or cross-namespace projections", () => {
  const machine = "22222222-2222-7222-8222-222222222222",
    profile = "33333333-3333-7333-8333-333333333333",
    identity = "44444444-4444-7444-8444-444444444444";
  const raw = () => ({
    ...value(),
    version: 3,
    migrated: true,
    machines: [{ id: machine, label: "Machine" }],
    hosts: [
      {
        ...host,
        machineId: machine,
        profile: { id: profile, label: "Profile", identityId: identity },
      },
    ],
  });
  expect(decodeHostCatalogue(raw()).hosts[0]?.profile).toEqual({
    id: profile,
    label: "Profile",
    identityId: identity,
  });
  for (const change of [
    { id },
    { identityId: profile },
    { identityId: "unknown" },
    { plaintext: "forbidden" },
  ]) {
    const r = raw();
    Object.assign(r.hosts[0]?.profile ?? {}, change);
    expect(() => decodeHostCatalogue(r)).toThrow();
  }
  expect(
    decodeHostCatalogue({
      ...value(),
      version: 3,
      migrated: false,
      machines: [],
      hosts: [{ ...host, machineId: null, profile: null }],
    }).hosts[0]?.profile,
  ).toBeUndefined();
});
it("v4 freezes explicit via and rejects foreign, self, missing or cross-kind references", () => {
  const profile = "33333333-3333-7333-8333-333333333333",
    other = "44444444-4444-7444-8444-444444444444",
    entrance = "55555555-5555-7555-8555-555555555555";
  const raw = {
    ...graphValue(),
    version: 4,
    hosts: [
      {
        ...host,
        machineId,
        profile: { id: profile, label: "Leaf", identityId: null, via: other },
      },
      {
        ...host,
        id: entrance,
        machineId,
        profile: { id: other, label: "Jump", identityId: null, via: null },
      },
    ],
  };
  expect(decodeHostCatalogue(raw).hosts[0]?.profile?.via).toBe(other);
  for (const via of [
    undefined,
    id,
    profile,
    "66666666-6666-7666-8666-666666666666",
  ])
    expect(() =>
      decodeHostCatalogue({
        ...raw,
        hosts: [
          { ...raw.hosts[0], profile: { ...raw.hosts[0]?.profile, via } },
          raw.hosts[1],
        ],
      }),
    ).toThrow("hosts.invalid");
});
it("deadline is uncertain, no automatic retry", async () => {
  vi.useFakeTimers();
  try {
    const rpc = vi.fn(() => new Promise<unknown>(() => {}));
    const pending = saveHost(
      token,
      token,
      { action: "remove", id },
      new AbortController().signal,
      rpc,
    );
    const receipt = expect(pending).rejects.toThrow("hosts.timeout");
    await vi.advanceTimersByTimeAsync(5000);
    await receipt;
    expect(rpc).toHaveBeenCalledTimes(1);
  } finally {
    vi.useRealTimers();
  }
});
