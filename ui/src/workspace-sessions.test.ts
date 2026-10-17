import { expect, it, vi } from "vitest";
import {
  decodeWorkspaceSessions,
  openWorkspaceProfile,
} from "./workspace-sessions";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
it("preserves exact u64 ids and rejects lossy or malformed catalogs", () => {
  expect(decodeWorkspaceSessions(["1", "18446744073709551615"])).toEqual([
    "1",
    "18446744073709551615",
  ]);
  expect(decodeWorkspaceSessions([])).toEqual([]);
  for (const value of [
    null,
    {},
    [1],
    ["01"],
    ["0"],
    ["18446744073709551616"],
    ["1", "1"],
    Array(65).fill("1"),
  ])
    expect(() => decodeWorkspaceSessions(value)).toThrow();
});
it("one explicit Profile request, no automatic retry on failure", async () => {
  const send = vi.fn(async () => "2");
  expect(await openWorkspaceProfile(nonce, "sh", send)).toBe("2");
  expect(send).toHaveBeenCalledExactlyOnceWith(nonce, "sh");
  send.mockRejectedValue(new Error("uncertain"));
  await expect(openWorkspaceProfile(nonce, "sh", send)).rejects.toThrow(
    "uncertain",
  );
  expect(send).toHaveBeenCalledTimes(2);
});
it("rejects bad nonce and program-like kind without native admission", async () => {
  const send = vi.fn(async () => "1");
  await expect(openWorkspaceProfile("bad", "sh", send)).rejects.toThrow();
  await expect(
    openWorkspaceProfile(nonce, "/bin/sh" as "sh", send),
  ).rejects.toThrow();
  expect(send).not.toHaveBeenCalled();
});
it("malformed launch response is not an empty/default session", async () => {
  await expect(
    openWorkspaceProfile(nonce, "sh", async () => 42),
  ).rejects.toThrow();
});
