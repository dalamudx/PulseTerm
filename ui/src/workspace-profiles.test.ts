import { describe, expect, it, vi } from "vitest";
import {
  decodeWorkspaceProfiles,
  loadWorkspaceProfiles,
} from "./workspace-profiles";

const nonce = "eb8c9c2a-5ca5-42a7-91f3-2950e523719b";
describe("read-only workspace profiles", () => {
  it("accepts the supported platform identities and explicit hidden state", () => {
    const values = [
      "bash",
      "zsh",
      "fish",
      "sh",
      "pwsh",
      "powershell",
      "cmd",
      "wsl",
    ].map((kind) => ({
      kind,
      hidden: false,
    }));
    expect(decodeWorkspaceProfiles(values)).toEqual(values);
    expect(decodeWorkspaceProfiles([])).toEqual([]);
    expect(
      decodeWorkspaceProfiles([{ kind: "bash", hidden: true }])[0]?.hidden,
    ).toBe(true);
  });
  it.each([
    null,
    {},
    { profiles: [] },
    [null],
    Array(1),
    [
      Object.assign(Object.create({ kind: "bash", hidden: false }), {
        a: 1,
        b: 2,
      }),
    ],
    ["bash"],
    [{ kind: "bash" }],
    [{ kind: "bash", hidden: 1 }],
    [{ kind: "unknown", hidden: false }],
    [{ kind: "bash", hidden: false, executable: "/secret" }],
    [
      { kind: "bash", hidden: false },
      { kind: "bash", hidden: true },
    ],
    Array(6).fill({ kind: "bash", hidden: false }),
  ])("fails closed on malformed response %j", (input) => {
    expect(() => decodeWorkspaceProfiles(input)).toThrow(
      "workspace.profiles_invalid",
    );
  });
  it("does not read a retired page or expose a late result", async () => {
    const controller = new AbortController();
    let finish: ((value: unknown) => void) | undefined;
    const result = loadWorkspaceProfiles(
      nonce,
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
      controller.signal,
    );
    controller.abort();
    expect(await result).toEqual({ status: "cancelled" });
    finish?.([{ kind: "bash", hidden: false }]);
    const send = vi.fn(async () => []);
    expect(await loadWorkspaceProfiles(nonce, send, controller.signal)).toEqual(
      { status: "cancelled" },
    );
    expect(send).not.toHaveBeenCalled();
  });
  it("rejects invalid nonce before native and bounds a stalled read without retry", async () => {
    const send = vi.fn(async () => []);
    const signal = new AbortController().signal;
    expect(await loadWorkspaceProfiles("bad", send, signal)).toEqual({
      status: "unavailable",
    });
    expect(send).not.toHaveBeenCalled();
    vi.useFakeTimers();
    try {
      const stalled = vi.fn(() => new Promise<unknown>(() => {}));
      const result = loadWorkspaceProfiles(nonce, stalled, signal);
      await vi.advanceTimersByTimeAsync(5_000);
      expect(await result).toEqual({ status: "unavailable" });
      expect(stalled).toHaveBeenCalledTimes(1);
      expect(vi.getTimerCount()).toBe(0);
    } finally {
      vi.useRealTimers();
    }
  });
  it("distinguishes empty results from backend and decode failures without reflecting errors", async () => {
    const signal = new AbortController().signal;
    expect(await loadWorkspaceProfiles(nonce, async () => [], signal)).toEqual({
      status: "ready",
      profiles: [],
    });
    expect(
      await loadWorkspaceProfiles(
        nonce,
        async () => {
          throw new Error("/secret/user");
        },
        signal,
      ),
    ).toEqual({ status: "unavailable" });
    expect(
      await loadWorkspaceProfiles(
        nonce,
        async () => [{ kind: "bash" }],
        signal,
      ),
    ).toEqual({ status: "unavailable" });
  });
});
