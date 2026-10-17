import { expect, it, vi } from "vitest";
import { checkConfigSource } from "./workspace-ssh-config";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222";
it.each(["unchanged", "changed"] as const)(
  "accepts only the versioned %s status",
  async (status) => {
    const rpc = vi.fn(async () => ({ version: 1, status }));
    expect(
      await checkConfigSource(nonce, token, new AbortController().signal, rpc),
    ).toBe(status);
    expect(rpc.mock.calls[0]).toEqual([
      "workspace_ssh_config_status",
      { nonce, tokenValue: token },
    ]);
  },
);
it.each([
  null,
  [],
  { version: 2, status: "unchanged" },
  { version: 1, status: "authorized" },
  { version: 1, status: "unchanged", host: "injected" },
  Object.assign(Object.create({ version: 1 }), { status: "unchanged" }),
])("rejects malformed source responses", async (raw) => {
  await expect(
    checkConfigSource(
      nonce,
      token,
      new AbortController().signal,
      async () => raw,
    ),
  ).rejects.toThrow("config.invalid");
});
it("does not execute response accessors", async () => {
  const getter = vi.fn(() => {
    throw Error("getter");
  });
  const raw = Object.defineProperty({ version: 1 }, "status", {
    get: getter,
    enumerable: true,
  });
  await expect(
    checkConfigSource(
      nonce,
      token,
      new AbortController().signal,
      async () => raw,
    ),
  ).rejects.toThrow("config.invalid");
  expect(getter).not.toHaveBeenCalled();
});
it("rejects a pre-canceled check before IPC", async () => {
  const stopped = new AbortController();
  stopped.abort();
  const rpc = vi.fn();
  await expect(
    checkConfigSource(nonce, token, stopped.signal, rpc),
  ).rejects.toThrow("config.cancelled");
  expect(rpc).not.toHaveBeenCalled();
});
