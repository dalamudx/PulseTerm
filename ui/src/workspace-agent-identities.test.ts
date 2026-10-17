import { afterEach, expect, it, vi } from "vitest";
import {
  decodeAgentCatalogue,
  readAgentCatalogue,
  saveAgentCatalogue,
} from "./workspace-agent-identities";
import { connectSavedAgentSsh } from "./workspace-ssh-session";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222",
  id = "33333333-3333-7333-8333-333333333333",
  host = "44444444-4444-7444-8444-444444444444",
  key = { algorithm: "ssh-ed25519", fingerprint: `SHA256:${"A".repeat(43)}` };
const catalogue = {
  version: 1,
  token,
  revision: "0",
  identities: [{ id, label: "Selected Agent", username: "agent-user", ...key }],
  keys: [key],
};
const pending = <T>() => {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
};
afterEach(() => {
  vi.useRealTimers();
  vi.restoreAllMocks();
});
it("rejects extra socket/secret data, duplicate IDs, noncanonical fingerprints and revision overflow", () => {
  for (const bad of [
    { ...catalogue, socket: "/unsafe" },
    { ...catalogue, revision: "9223372036854775808" },
    {
      ...catalogue,
      identities: [...catalogue.identities, ...catalogue.identities],
    },
    { ...catalogue, keys: [key, key] },
    { ...catalogue, keys: [{ ...key, fingerprint: `${key.fingerprint}=` }] },
  ])
    expect(() => decodeAgentCatalogue(bad)).toThrow();
  expect(decodeAgentCatalogue(catalogue).identities[0]?.id).toBe(id);
  expect(
    decodeAgentCatalogue({
      ...catalogue,
      keys: [{ ...key, algorithm: "sk-ssh-ed25519" + "@" + "openssh.com" }],
    }).keys,
  ).toHaveLength(1);
});
it("saves only explicit discovered-key index and normalized metadata, never address or signature", async () => {
  const rpc = vi.fn(async () => ({ ...catalogue, revision: "1" }));
  await saveAgentCatalogue(
    nonce,
    decodeAgentCatalogue(catalogue),
    {
      action: "create",
      key: 0,
      label: "Selected Agent",
      username: "agent-user",
    },
    new AbortController().signal,
    rpc,
  );
  expect(rpc).toHaveBeenCalledExactlyOnceWith("workspace_agent_identity_save", {
    nonce,
    token,
    change: {
      operation: "create",
      key: 0,
      label: "Selected Agent",
      username: "agent-user",
    },
  });
  expect(() =>
    decodeAgentCatalogue({
      ...catalogue,
      keys: [{ ...key, algorithm: "unknown" }],
    }),
  ).toThrow();
  await expect(
    saveAgentCatalogue(
      nonce,
      decodeAgentCatalogue(catalogue),
      { action: "create", key: 1, label: "A", username: "agent-user" },
      new AbortController().signal,
      rpc,
    ),
  ).rejects.toThrow();
  expect(rpc).toHaveBeenCalledTimes(1);
});
it("cancelled discovery rejects immediately and cancels late original-page receipt", async () => {
  const reply = pending<unknown>(),
    control = new AbortController();
  const rpc = vi.fn(async (command: string) =>
    command === "workspace_agent_identities" ? reply.promise : null,
  );
  const result = readAgentCatalogue(nonce, control.signal, true, rpc);
  const rejection = expect(result).rejects.toThrow("cancelled");
  control.abort();
  await rejection;
  reply.resolve(catalogue);
  await vi.waitFor(() =>
    expect(rpc).toHaveBeenCalledWith("workspace_agent_identity_cancel", {
      nonce,
      token,
    }),
  );
  expect(rpc.mock.calls.map((c) => c[0])).toEqual([
    "workspace_agent_identities",
    "workspace_agent_identity_cancel",
  ]);
});
it("cancel while saving revokes original ticket and any late successor", async () => {
  const reply = pending<unknown>(),
    control = new AbortController(),
    next = "55555555-5555-4555-8555-555555555555";
  const rpc = vi.fn(async (command: string) =>
    command === "workspace_agent_identity_save" ? reply.promise : null,
  );
  const result = saveAgentCatalogue(
    nonce,
    decodeAgentCatalogue(catalogue),
    { action: "remove", id },
    control.signal,
    rpc,
  );
  const rejected = expect(result).rejects.toThrow("cancelled");
  control.abort();
  await rejected;
  reply.resolve({ ...catalogue, token: next, identities: [] });
  await vi.waitFor(() =>
    expect(rpc).toHaveBeenCalledWith("workspace_agent_identity_cancel", {
      nonce,
      token: next,
    }),
  );
  expect(rpc).toHaveBeenCalledWith("workspace_agent_identity_cancel", {
    nonce,
    token,
  });
});
it("Agent session uses four Agent operations and never password/private-key fallback", async () => {
  const selection = { host, identity: id };
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("_begin")
      ? { version: 1, token }
      : command.endsWith("_start")
        ? "9"
        : null,
  );
  await expect(
    connectSavedAgentSsh(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      rpc,
    ),
  ).resolves.toBe("9");
  expect(rpc.mock.calls.map((c) => c[0])).toEqual([
    "workspace_ssh_agent_session_begin",
    "workspace_ssh_agent_session_prepare",
    "workspace_ssh_agent_session_start",
  ]);
  const refused = vi.fn(async (command: string) => {
    if (command.endsWith("_prepare")) throw Error("refused");
    return command.endsWith("_begin") ? { version: 1, token } : null;
  });
  await expect(
    connectSavedAgentSsh(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      refused,
    ),
  ).rejects.toThrow("refused");
  expect(refused.mock.calls.map((c) => c[0])).toEqual([
    "workspace_ssh_agent_session_begin",
    "workspace_ssh_agent_session_prepare",
    "workspace_ssh_agent_session_cancel",
  ]);
});
