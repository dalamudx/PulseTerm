import { invoke } from "@tauri-apps/api/core";
import { describe, expect, it, vi } from "vitest";
import {
  decodeRecentHosts,
  requestRecentHosts,
} from "./workspace-recent-hosts";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn() }));
const id = (n: number) =>
  `019a9c6d-7000-7000-8000-${n.toString(16).padStart(12, "0")}`;
describe("recent saved host projection", () => {
  it("preserves backend MRU order even when wall time moved backwards", () => {
    expect(
      decodeRecentHosts([
        { host: id(2), connectedAt: 900 },
        { host: id(1), connectedAt: 1000 },
      ]).map((r) => r.host),
    ).toEqual([id(2), id(1)]);
  });
  it("rejects unknown fields, duplicate targets, raw addresses and invalid times", () => {
    for (const raw of [
      [{ host: id(1), connectedAt: 1000, credential: "no" }],
      [{ host: id(1), connectedAt: 0 }],
      [{ host: "user@host:22", connectedAt: 1000 }],
      [{ host: id(1), connectedAt: Number.MAX_SAFE_INTEGER + 1 }],
      [
        { host: id(1), connectedAt: 1000 },
        { host: id(1), connectedAt: 1001 },
      ],
      Array.from({ length: 33 }, (_, n) => ({
        host: id(n),
        connectedAt: 1000,
      })),
    ])
      expect(() => decodeRecentHosts(raw)).toThrow("recent.invalid");
  });
  it("late page reads cannot publish or cause a write after cancellation", async () => {
    let reply: ((v: unknown) => void) | undefined;
    vi.mocked(invoke).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          reply = resolve;
        }),
    );
    const life = new AbortController();
    const pending = requestRecentHosts("original-page", life.signal);
    life.abort();
    reply?.([{ host: id(1), connectedAt: 1000 }]);
    await expect(pending).rejects.toThrow("recent.cancelled");
    expect(invoke).toHaveBeenLastCalledWith("workspace_recent_hosts", {
      nonce: "original-page",
    });
  });
});
