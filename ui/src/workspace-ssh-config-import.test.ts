import { afterEach, expect, it, vi } from "vitest";
import {
  decodeConfigImport,
  prepareConfigImport,
  saveConfigImport,
} from "./workspace-ssh-config-import";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  previewToken = "9f3da39f-e375-4692-8390-814d18191660",
  importToken = "03969d3e-a25e-4a50-a8c4-f1d5c03e6aa6",
  hostId = "dddddddd-dddd-7ddd-8ddd-dddddddddddd";
const raw = () => ({
  version: 1,
  token: importToken,
  label: "Imported prod",
  host: "example.test",
  port: 2222,
  username: "alice",
  identityFileCount: 1,
});
afterEach(() => vi.useRealTimers());
it("decodes only fixed target metadata and label", () => {
  expect(Object.isFrozen(decodeConfigImport(raw()))).toBe(true);
  expect(() => decodeConfigImport({ ...raw(), secret: "unexpected" })).toThrow(
    "config.invalid",
  );
});
it.each(["", " leading", "trailing ", "bad\n", "bad\u202e", "é".repeat(129)])(
  "rejects invalid name %j before native work",
  async (label) => {
    const rpc = vi.fn();
    await expect(
      prepareConfigImport(
        nonce,
        previewToken,
        label,
        new AbortController().signal,
        rpc,
      ),
    ).rejects.toThrow("config.invalid");
    expect(rpc).not.toHaveBeenCalled();
  },
);
it("prepares only with original preview ticket and returns a distinct import ticket", async () => {
  const rpc = vi.fn(async () => raw());
  const prepared = await prepareConfigImport(
    nonce,
    previewToken,
    "Imported prod",
    new AbortController().signal,
    rpc,
  );
  expect(prepared.token).toBe(importToken);
  expect(rpc).toHaveBeenCalledWith("workspace_ssh_config_import_prepare", {
    nonce,
    tokenValue: previewToken,
    label: "Imported prod",
  });
});
it("aborted request sends no prepare or save", async () => {
  const rpc = vi.fn(),
    abort = new AbortController();
  abort.abort();
  await expect(
    prepareConfigImport(
      nonce,
      previewToken,
      "Imported prod",
      abort.signal,
      rpc,
    ),
  ).rejects.toThrow("config.cancelled");
  await expect(
    saveConfigImport(nonce, importToken, abort.signal, rpc),
  ).rejects.toThrow("config.cancelled");
  expect(rpc).not.toHaveBeenCalled();
});
it("cancels late prepare receipt after closing", async () => {
  let resolve!: (v: unknown) => void;
  const rpc = vi.fn((command: string) =>
    command.endsWith("prepare")
      ? new Promise((r) => {
          resolve = r;
        })
      : Promise.resolve(null),
  );
  const abort = new AbortController();
  const pending = prepareConfigImport(
    nonce,
    previewToken,
    "Imported prod",
    abort.signal,
    rpc,
  );
  const rejection = expect(pending).rejects.toThrow("config.cancelled");
  abort.abort();
  await rejection;
  resolve(raw());
  await Promise.resolve();
  await Promise.resolve();
  expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_config_import_cancel", {
    nonce,
    tokenValue: importToken,
  });
  expect(rpc).toHaveBeenCalledTimes(2);
});
it("cancels late prepare after five-second uncertain deadline", async () => {
  vi.useFakeTimers();
  let resolve!: (v: unknown) => void;
  const rpc = vi.fn((command: string) =>
    command.endsWith("prepare")
      ? new Promise((r) => {
          resolve = r;
        })
      : Promise.resolve(null),
  );
  const pending = prepareConfigImport(
    nonce,
    previewToken,
    "Imported prod",
    new AbortController().signal,
    rpc,
  );
  const rejection = expect(pending).rejects.toThrow("config.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await rejection;
  resolve(raw());
  await Promise.resolve();
  await Promise.resolve();
  expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_config_import_cancel", {
    nonce,
    tokenValue: importToken,
  });
});
it("malformed prepare projection with a valid token is revoked", async () => {
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("prepare") ? { ...raw(), port: 0 } : null,
  );
  await expect(
    prepareConfigImport(
      nonce,
      previewToken,
      "Imported prod",
      new AbortController().signal,
      rpc,
    ),
  ).rejects.toThrow("config.invalid");
  expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_config_import_cancel", {
    nonce,
    tokenValue: importToken,
  });
});
it("save sends no target fields and requires persisted host ID", async () => {
  const rpc = vi.fn(async () => hostId);
  expect(
    await saveConfigImport(
      nonce,
      importToken,
      new AbortController().signal,
      rpc,
    ),
  ).toBe(hostId);
  expect(rpc).toHaveBeenCalledWith("workspace_ssh_config_import_save", {
    nonce,
    tokenValue: importToken,
  });
});
it("save cancellation revokes its own running approval", async () => {
  const rpc = vi.fn((command: string) =>
    command.endsWith("save") ? new Promise(() => {}) : Promise.resolve(null),
  );
  const abort = new AbortController();
  const pending = saveConfigImport(nonce, importToken, abort.signal, rpc);
  const rejection = expect(pending).rejects.toThrow("config.cancelled");
  abort.abort();
  await rejection;
  expect(rpc).toHaveBeenCalledTimes(2);
  expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_config_import_cancel", {
    nonce,
    tokenValue: importToken,
  });
});
it("save timeout is uncertain and cannot retry", async () => {
  vi.useFakeTimers();
  const rpc = vi.fn((command: string) =>
    command.endsWith("save") ? new Promise(() => {}) : Promise.resolve(null),
  );
  const pending = saveConfigImport(
    nonce,
    importToken,
    new AbortController().signal,
    rpc,
  );
  const rejection = expect(pending).rejects.toThrow("config.timeout");
  await vi.advanceTimersByTimeAsync(5000);
  await rejection;
  expect(rpc).toHaveBeenCalledTimes(2);
  expect(rpc).toHaveBeenLastCalledWith("workspace_ssh_config_import_cancel", {
    nonce,
    tokenValue: importToken,
  });
});
it("source or SQL conflict does not retry writes", async () => {
  const rpc = vi.fn(async () => {
    throw Error("workspace.config_conflict");
  });
  await expect(
    saveConfigImport(nonce, importToken, new AbortController().signal, rpc),
  ).rejects.toThrow("workspace.config_conflict");
  expect(rpc).toHaveBeenCalledTimes(1);
});
it.each([null, true, importToken])(
  "invalid save receipt %j cannot establish persistence",
  async (raw) => {
    await expect(
      saveConfigImport(
        nonce,
        importToken,
        new AbortController().signal,
        async () => raw,
      ),
    ).rejects.toThrow("config.invalid");
  },
);
