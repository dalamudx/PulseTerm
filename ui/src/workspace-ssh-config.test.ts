import { afterEach, expect, it, vi } from "vitest";
import {
  confirmConfigPreview,
  decodeConfigPreview,
  readConfigPreview,
} from "./workspace-ssh-config";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const raw = () => ({
  version: 1,
  token: nonce,
  host: "demo.example",
  port: 22,
  username: "alice",
  identityFileCount: 32,
});
afterEach(() => vi.useRealTimers());
it("accepts exactly bounded drawing metadata, including the backend's 32 hint limit", () => {
  const result = decodeConfigPreview(raw());
  expect(result.identityFileCount).toBe(32);
  expect(Object.isFrozen(result)).toBe(true);
});
it.each([
  { version: 2 },
  { token: nonce.toUpperCase() },
  { host: "Demo.example" },
  { host: "<img>" },
  { port: 0 },
  { port: 65536 },
  { port: 1.1 },
  { username: "alice\n" },
  { username: "alice\u202e" },
  { username: "é".repeat(128) },
  { username: "-alice" },
  { identityFileCount: 33 },
  { identityFileCount: -1 },
  { identityFileCount: 0.5 },
  { privateKey: "not-a-projection-field" },
])("rejects malformed or extended projection %j", (patch) => {
  expect(() => decodeConfigPreview({ ...raw(), ...patch })).toThrow(
    "config.invalid",
  );
});
it("calls only the read command with a fixed nonce and case-sensitive alias", async () => {
  const rpc = vi.fn(async () => raw());
  await readConfigPreview(nonce, "Prod", new AbortController().signal, rpc);
  expect(rpc).toHaveBeenCalledWith("workspace_ssh_config_preview", {
    nonce,
    alias: "Prod",
  });
});
it.each(["", "*.example", "a b", "../../config", "prod\n", "a".repeat(254)])(
  "invalid alias %j reaches no native command",
  async (alias) => {
    const rpc = vi.fn();
    await expect(
      readConfigPreview(nonce, alias, new AbortController().signal, rpc),
    ).rejects.toThrow();
    expect(rpc).not.toHaveBeenCalled();
  },
);
it("an already cancelled read reaches no native command", async () => {
  const abort = new AbortController();
  abort.abort();
  const rpc = vi.fn();
  await expect(
    readConfigPreview(nonce, "prod", abort.signal, rpc),
  ).rejects.toThrow("config.cancelled");
  expect(rpc).not.toHaveBeenCalled();
});
it("abort rejects a pending read and never returns its late metadata", async () => {
  let finish!: (v: unknown) => void;
  const rpc = vi.fn(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const abort = new AbortController();
  const result = readConfigPreview(nonce, "prod", abort.signal, rpc);
  const rejected = expect(result).rejects.toThrow("config.cancelled");
  abort.abort();
  await rejected;
  finish(raw());
  await Promise.resolve();
  expect(rpc).toHaveBeenCalledTimes(1);
});
it("uncertain confirmation times out once without retry", async () => {
  vi.useFakeTimers();
  const rpc = vi.fn(() => new Promise<unknown>(() => {}));
  const result = confirmConfigPreview(
    nonce,
    nonce,
    new AbortController().signal,
    rpc,
  );
  const rejected = expect(result).rejects.toThrow("config.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await rejected;
  expect(rpc).toHaveBeenCalledTimes(1);
  expect(rpc).toHaveBeenCalledWith("workspace_ssh_config_confirm", {
    nonce,
    tokenValue: nonce,
  });
});
it.each([true, {}, undefined])(
  "non-unit confirmation %j is rejected",
  async (raw) => {
    await expect(
      confirmConfigPreview(
        nonce,
        nonce,
        new AbortController().signal,
        async () => raw,
      ),
    ).rejects.toThrow("config.invalid");
  },
);
it("native source conflict is not retried", async () => {
  const rpc = vi.fn(async () => {
    throw Error("workspace.config_conflict");
  });
  await expect(
    confirmConfigPreview(nonce, nonce, new AbortController().signal, rpc),
  ).rejects.toThrow("workspace.config_conflict");
  expect(rpc).toHaveBeenCalledTimes(1);
});
