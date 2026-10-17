import { afterEach, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

import {
  decodeTemporaryTarget,
  prepareTemporaryTarget,
  saveTemporaryTarget,
} from "./workspace-temporary-target";

afterEach(() => invoke.mockReset());
const token = "01010101-0101-4101-8101-010101010101";
const id = "019a9c6d-7000-7000-8000-000000000001";
const raw = () => ({
  version: 1,
  token,
  target: {
    id,
    label: "临时主机",
    host: "127.0.0.1",
    port: 22,
    username: "user",
  },
});
const fields = {
  label: "临时主机",
  host: "127.0.0.1",
  port: 22,
  username: "user",
};
it("accepts only a bounded canonical metadata target with no extra execution fields", () => {
  expect(decodeTemporaryTarget(raw()).target).toEqual({ id, ...fields });
  for (const value of [
    { ...raw(), command: "sh" },
    { ...raw(), version: 2 },
    { ...raw(), token: id },
    { ...raw(), target: { ...raw().target, password: "secret" } },
    { ...raw(), target: { ...raw().target, id: token } },
  ])
    expect(() => decodeTemporaryTarget(value)).toThrow();
});
it("cancels a native target issued after the initiating form closed", async () => {
  let resolve: (value: unknown) => void = () => {};
  invoke
    .mockImplementationOnce(
      () =>
        new Promise((done) => {
          resolve = done;
        }),
    )
    .mockResolvedValueOnce(undefined);
  const life = new AbortController();
  const pending = prepareTemporaryTarget(token, fields, life.signal);
  const rejected = expect(pending).rejects.toThrow("cancelled");
  life.abort();
  resolve(raw());
  await rejected;
  expect(invoke).toHaveBeenLastCalledWith("workspace_quick_target_cancel", {
    nonce: token,
    token,
  });
});
it("saves only the frozen target receipt and explicit name, rejecting unconfirmed replies", async () => {
  const target = decodeTemporaryTarget(raw());
  invoke.mockResolvedValueOnce({ version: 1, host: id });
  await expect(
    saveTemporaryTarget(
      token,
      target,
      "保存名称",
      new AbortController().signal,
    ),
  ).resolves.toBe(id);
  expect(invoke).toHaveBeenCalledWith("workspace_quick_target_save", {
    nonce: token,
    token,
    label: "保存名称",
  });
  invoke.mockResolvedValueOnce({ version: 1, host: token });
  await expect(
    saveTemporaryTarget(
      token,
      target,
      "保存名称",
      new AbortController().signal,
    ),
  ).rejects.toThrow();
});
