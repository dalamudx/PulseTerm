import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const native = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: native }));

import { saveIdentityPassword } from "./workspace-identity-password";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  identity = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb",
  ticket = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
const signal = () => new AbortController();
beforeEach(() => {
  native.mockReset();
});
afterEach(() => vi.useRealTimers());
describe("receive-only identity password", () => {
  it("only metadata identity/ticket and bounded owned bytes; zeroes input/wire on success", async () => {
    let captured: unknown;
    native.mockImplementation(
      (command: string, args: Record<string, unknown>) => {
        if (command.endsWith("prepare"))
          return Promise.resolve({ version: 1, ticket });
        if (command.endsWith("submit")) {
          captured = [...(args.password as number[])];
          return Promise.resolve({ version: 1 });
        }
        return Promise.resolve();
      },
    );
    const bytes = new TextEncoder().encode("fixed-secret");
    await saveIdentityPassword(nonce, identity, bytes, signal().signal);
    expect(captured).toEqual(
      Array.from(new TextEncoder().encode("fixed-secret")),
    );
    expect(bytes.every((b) => b === 0)).toBe(true);
    expect(native.mock.calls[0]?.[1]).toEqual({ nonce, identity });
    expect(Object.keys(native.mock.calls[1]?.[1] ?? {}).sort()).toEqual([
      "nonce",
      "password",
      "ticket",
    ]);
    const submitted = native.mock.calls[1];
    if (!submitted) throw Error("fixture.submit_missing");
    expect((submitted[1].password as number[]).every((b) => b === 0)).toBe(
      true,
    );
  });
  it.each([0, 4097])(
    "rejects byte budget %s without native admission",
    async (length) => {
      const bytes = new Uint8Array(length).fill(7);
      await expect(
        saveIdentityPassword(nonce, identity, bytes, signal().signal),
      ).rejects.toThrow();
      expect(native).not.toHaveBeenCalled();
      expect(bytes.every((b) => b === 0)).toBe(true);
    },
  );
  it("late prepare after cancel cannot submit and retires ticket", async () => {
    let finish: (value: unknown) => void = () => {};
    native.mockImplementation((command: string) =>
      command.endsWith("prepare")
        ? new Promise((resolve) => {
            finish = resolve;
          })
        : Promise.resolve(),
    );
    const controller = signal();
    const bytes = new Uint8Array([1, 2, 3]);
    const pending = saveIdentityPassword(
      nonce,
      identity,
      bytes,
      controller.signal,
    );
    controller.abort();
    await expect(pending).rejects.toThrow("cancelled");
    finish({ version: 1, ticket });
    await Promise.resolve();
    await Promise.resolve();
    expect(
      native.mock.calls.some(
        (call) => call[0] === "workspace_identity_password_submit",
      ),
    ).toBe(false);
    expect(
      native.mock.calls.some(
        (call) => call[0] === "workspace_identity_password_cancel",
      ),
    ).toBe(true);
    expect([...bytes]).toEqual([0, 0, 0]);
  });
  it("unknown submit timeout cancels once and never retries write", async () => {
    vi.useFakeTimers();
    native.mockImplementation((command: string) =>
      command.endsWith("prepare")
        ? Promise.resolve({ version: 1, ticket })
        : command.endsWith("submit")
          ? new Promise(() => {})
          : Promise.resolve(),
    );
    const bytes = new Uint8Array([1]);
    const pending = saveIdentityPassword(
      nonce,
      identity,
      bytes,
      signal().signal,
    );
    const rejection = expect(pending).rejects.toThrow("uncertain");
    await vi.advanceTimersByTimeAsync(5000);
    await rejection;
    expect(
      native.mock.calls.filter(
        (call) => call[0] === "workspace_identity_password_submit",
      ),
    ).toHaveLength(1);
    expect(
      native.mock.calls.filter(
        (call) => call[0] === "workspace_identity_password_cancel",
      ),
    ).toHaveLength(1);
    expect([...bytes]).toEqual([0]);
  });
  it("rejects extra secret response fields without retry or echo", async () => {
    native
      .mockResolvedValueOnce({ version: 1, ticket })
      .mockResolvedValueOnce({ version: 1, password: "must-not-echo" })
      .mockResolvedValue(undefined);
    await expect(
      saveIdentityPassword(
        nonce,
        identity,
        new Uint8Array([1]),
        signal().signal,
      ),
    ).rejects.toThrow("protocol");
    expect(
      native.mock.calls.filter(
        (call) => call[0] === "workspace_identity_password_submit",
      ),
    ).toHaveLength(1);
  });
});
