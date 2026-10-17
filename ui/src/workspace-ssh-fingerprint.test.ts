import { describe, expect, it, vi } from "vitest";
import type { HostRpc } from "./workspace-hosts";
import {
  decodeFingerprint,
  FingerprintTicket,
} from "./workspace-ssh-fingerprint";

const nonce = "00000000-0000-4000-8000-000000000001",
  token = "00000000-0000-4000-8000-000000000002",
  host = "00000000-0000-7000-8000-000000000003";
const dto = (status: "unknown" | "changed" | "already_pinned" = "unknown") => ({
  version: 1,
  token: status === "unknown" ? token : null,
  label: "固定测试",
  host: "127.0.0.1",
  port: 22,
  algorithm: "ssh-ed25519",
  fingerprint: `SHA256:${"A".repeat(43)}`,
  status,
  source: "application_only",
});
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function fixture(
  observe: Promise<unknown> = Promise.resolve(dto()),
  confirm: Promise<unknown> = Promise.resolve({ version: 1, revision: "1" }),
) {
  const calls: Readonly<{ command: string; args: Record<string, unknown> }>[] =
    [];
  const rpc: HostRpc = async (command, args) => {
    calls.push({ command, args });
    if (command.endsWith("prepare")) return { version: 1, token };
    if (command.endsWith("observe")) return observe;
    if (command.endsWith("confirm")) return confirm;
    return undefined;
  };
  return { rpc, calls };
}
describe("original-page fingerprint ticket adapter", () => {
  it("strict public DTO never accepts changed key approval, other ticket, extra credential, control text or malformed digest", () => {
    expect(decodeFingerprint(dto(), token).status).toBe("unknown");
    for (const changed of [
      null,
      [],
      { ...dto(), version: 2 },
      { ...dto(), credential: "secret" },
      { ...dto(), token: nonce },
      { ...dto("changed"), token },
      { ...dto(), label: "bad\u001b" },
      { ...dto(), port: 0 },
      { ...dto(), algorithm: "ssh ed25519" },
      { ...dto(), fingerprint: "SHA256:short" },
      { ...dto(), source: "system" },
    ])
      expect(() => decodeFingerprint(changed, token)).toThrow(
        "fingerprint.invalid",
      );
  });
  it("only selected saved host/nonce and backend opaque token are sent; observe and confirm are one-shot", async () => {
    const f = fixture(),
      scope = new AbortController();
    const client = await FingerprintTicket.prepare(
      nonce,
      host,
      scope.signal,
      f.rpc,
    );
    await client.observe();
    await expect(client.observe()).rejects.toThrow();
    expect(await client.confirm()).toBe("1");
    await expect(client.confirm()).rejects.toThrow();
    expect(f.calls).toEqual([
      { command: "workspace_ssh_fingerprint_prepare", args: { nonce, host } },
      { command: "workspace_ssh_fingerprint_observe", args: { nonce, token } },
      { command: "workspace_ssh_fingerprint_confirm", args: { nonce, token } },
    ]);
  });
  it.each(["changed", "already_pinned"] as const)(
    "%s offers no confirmation request",
    async (status) => {
      const f = fixture(Promise.resolve(dto(status))),
        scope = new AbortController();
      const client = await FingerprintTicket.prepare(
        nonce,
        host,
        scope.signal,
        f.rpc,
      );
      expect((await client.observe()).status).toBe(status);
      await expect(client.confirm()).rejects.toThrow(
        "fingerprint.not_approved",
      );
      expect(f.calls.some((c) => c.command.endsWith("confirm"))).toBe(false);
    },
  );
  it("closing while prepare waits cancels its late original ticket exactly once", async () => {
    const pending = deferred<unknown>(),
      calls: string[] = [];
    const scope = new AbortController();
    const rpc: HostRpc = async (command) => {
      calls.push(command);
      return command.endsWith("prepare") ? pending.promise : undefined;
    };
    const preparing = FingerprintTicket.prepare(nonce, host, scope.signal, rpc);
    scope.abort();
    await expect(preparing).rejects.toThrow("fingerprint.cancelled");
    pending.resolve({ version: 1, token });
    await Promise.resolve();
    await Promise.resolve();
    expect(calls).toEqual([
      "workspace_ssh_fingerprint_prepare",
      "workspace_ssh_fingerprint_cancel",
    ]);
  });
  it("cancel while observing cannot apply a late fingerprint and uses original token", async () => {
    const pending = deferred<unknown>(),
      f = fixture(pending.promise),
      scope = new AbortController();
    const client = await FingerprintTicket.prepare(
      nonce,
      host,
      scope.signal,
      f.rpc,
    );
    const observing = client.observe();
    scope.abort();
    await expect(observing).rejects.toThrow("fingerprint.cancelled");
    pending.resolve(dto());
    await Promise.resolve();
    expect(f.calls.filter((c) => c.command.endsWith("cancel"))).toEqual([
      { command: "workspace_ssh_fingerprint_cancel", args: { nonce, token } },
    ]);
    await expect(client.confirm()).rejects.toThrow();
  });
  it("cancel while confirming revokes its original ticket and never retries an uncertain write", async () => {
    const pending = deferred<unknown>(),
      f = fixture(Promise.resolve(dto()), pending.promise),
      scope = new AbortController();
    const client = await FingerprintTicket.prepare(
      nonce,
      host,
      scope.signal,
      f.rpc,
    );
    await client.observe();
    const confirmation = client.confirm();
    scope.abort();
    await expect(confirmation).rejects.toThrow("fingerprint.cancelled");
    pending.resolve({ version: 1, revision: "1" });
    await Promise.resolve();
    await expect(client.confirm()).rejects.toThrow();
    expect(f.calls.filter((c) => c.command.endsWith("confirm"))).toHaveLength(
      1,
    );
    expect(f.calls.filter((c) => c.command.endsWith("cancel"))).toHaveLength(1);
  });
  it("bad incoming metadata cancels instead of publishing or sending auth", async () => {
    const f = fixture(Promise.resolve({ ...dto(), rawKey: "untrusted" })),
      scope = new AbortController();
    const client = await FingerprintTicket.prepare(
      nonce,
      host,
      scope.signal,
      f.rpc,
    );
    await expect(client.observe()).rejects.toThrow("fingerprint.invalid");
    expect(f.calls.filter((c) => c.command.endsWith("cancel"))).toHaveLength(1);
  });
  it("timeout sends one cancellation, rejects late receipt and does not extend fixed ready expiry", async () => {
    vi.useFakeTimers();
    try {
      const pending = deferred<unknown>(),
        f = fixture(pending.promise),
        scope = new AbortController();
      const client = await FingerprintTicket.prepare(
        nonce,
        host,
        scope.signal,
        f.rpc,
      );
      const observed = expect(client.observe()).rejects.toThrow(
        "fingerprint.timeout",
      );
      await vi.advanceTimersByTimeAsync(12000);
      await observed;
      pending.resolve(dto());
      await Promise.resolve();
      expect(f.calls.filter((c) => c.command.endsWith("cancel"))).toHaveLength(
        1,
      );
    } finally {
      vi.useRealTimers();
    }
  });
});
