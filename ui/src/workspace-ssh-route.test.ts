import { expect, it, vi } from "vitest";
import type { HostRpc } from "./workspace-hosts";
import {
  connectSshRoute,
  continueSshRoute,
  decodeRouteReply,
} from "./workspace-ssh-route";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222";
const selection = {
  host: "11111111-1111-7111-8111-111111111111",
  identity: "22222222-2222-7222-8222-222222222222",
  profile: "33333333-3333-7333-8333-333333333333",
  privateKey: false,
};
function prompt(index = 0, count = 2) {
  return {
    state: "confirm",
    version: 1,
    challenge: index === 0 ? nonce : token,
    prompt: {
      index,
      count,
      entrance:
        index === 0 ? "44444444-4444-7444-8444-444444444444" : selection.host,
      label: "fixed target",
      host: "127.0.0.1",
      port: 22,
      username: "fixed-user",
      identity: selection.identity,
      identityLabel: "Selected identity",
      algorithm: "ssh-ed25519",
      fingerprint: `SHA256:${"A".repeat(43)}`,
      firstUse: index === 0,
    },
  };
}
function deferred<T>() {
  let resolve: (v: T) => void = () => {};
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { resolve, promise };
}
it("freezes only saved IDs and consumes each actual-peer challenge once in order", async () => {
  let index = 0;
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("begin")
      ? { version: 1, token }
      : command.endsWith("wait")
        ? prompt(0)
        : command.endsWith("confirm")
          ? ++index === 1
            ? prompt(1)
            : { state: "connected", version: 1, session: "9007199254740993" }
          : null,
  );
  const confirm = vi.fn(async () => true);
  expect(
    await connectSshRoute(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      confirm,
      rpc as HostRpc,
    ),
  ).toBe("9007199254740993");
  expect(confirm.mock.calls.length).toBe(2);
  expect(rpc.mock.calls.map(([command]) => command)).toEqual([
    "workspace_ssh_route_begin",
    "workspace_ssh_route_wait",
    "workspace_ssh_route_confirm",
    "workspace_ssh_route_confirm",
  ]);
  expect(rpc).toHaveBeenNthCalledWith(1, "workspace_ssh_route_begin", {
    nonce,
    selection,
  });
  expect(rpc).toHaveBeenNthCalledWith(3, "workspace_ssh_route_confirm", {
    nonce,
    token,
    challenge: nonce,
    approved: true,
  });
});
it.each([
  "repeated challenge",
  "changed count",
  "out of order",
  "connected without challenge",
])(
  "rejects %s and cancels without another authentication attempt",
  async (variant) => {
    const rpc = vi.fn(async (command: string) => {
      if (command.endsWith("begin")) return { version: 1, token };
      if (command.endsWith("wait"))
        return variant === "connected without challenge"
          ? { state: "connected", version: 1, session: "1" }
          : prompt(0);
      if (command.endsWith("confirm")) {
        const next = prompt(1);
        if (variant === "repeated challenge") next.challenge = nonce;
        if (variant === "changed count") next.prompt.count = 3;
        if (variant === "out of order") next.prompt.index = 0;
        return next;
      }
      return null;
    });
    await expect(
      connectSshRoute(
        nonce,
        selection,
        new AbortController().signal,
        () => true,
        async () => true,
        rpc as HostRpc,
      ),
    ).rejects.toThrow("ssh.route_protocol");
    expect(rpc.mock.calls.filter(([c]) => c.endsWith("cancel"))).toHaveLength(
      1,
    );
    expect(rpc.mock.calls.filter(([c]) => c.endsWith("begin"))).toHaveLength(1);
  },
);
it("late begin after abort is revoked once and never starts waiting for a peer", async () => {
  const issued = deferred<unknown>();
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("begin") ? issued.promise : null,
  );
  const life = new AbortController();
  const result = connectSshRoute(
    nonce,
    selection,
    life.signal,
    () => true,
    async () => true,
    rpc as HostRpc,
  );
  const rejected = expect(result).rejects.toThrow("ssh.route_cancelled");
  life.abort();
  await rejected;
  issued.resolve({ version: 1, token });
  await issued.promise;
  await Promise.resolve();
  expect(rpc.mock.calls.map(([c]) => c)).toEqual([
    "workspace_ssh_route_begin",
    "workspace_ssh_route_cancel",
  ]);
});
it("navigation while a prompt is open revokes without submitting approval", async () => {
  let current = true;
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("begin")
      ? { version: 1, token }
      : command.endsWith("wait")
        ? prompt()
        : null,
  );
  await expect(
    connectSshRoute(
      nonce,
      selection,
      new AbortController().signal,
      () => current,
      async () => {
        current = false;
        return true;
      },
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_cancelled");
  expect(rpc.mock.calls.map(([c]) => c)).toEqual([
    "workspace_ssh_route_begin",
    "workspace_ssh_route_wait",
    "workspace_ssh_route_cancel",
  ]);
});
it("declining first pin cancels rather than saving or trying an identity", async () => {
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("begin")
      ? { version: 1, token }
      : command.endsWith("wait")
        ? prompt()
        : null,
  );
  await expect(
    connectSshRoute(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      async () => false,
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_cancelled");
  expect(rpc.mock.calls.filter(([c]) => c.endsWith("confirm"))).toHaveLength(0);
  expect(rpc.mock.calls.filter(([c]) => c.endsWith("cancel"))).toHaveLength(1);
});
it("rejects raw addresses, malformed algorithms, over-limit hops, and injected fields", () => {
  const p = prompt();
  for (const changed of [
    { ...p, prompt: { ...p.prompt, count: 10 } },
    { ...p, prompt: { ...p.prompt, algorithm: "ssh-ed25519\n" } },
    { ...p, prompt: { ...p.prompt, fingerprint: "not-a-fingerprint" } },
    { ...p, prompt: { ...p.prompt, password: "injected" } },
    { ...p, session: "1" },
  ])
    expect(() => decodeRouteReply(changed)).toThrow("ssh.route_protocol");
});

it("preserves a frozen Agent leaf selection through the actual route RPC", async () => {
  const chosen = { ...selection, agent: true };
  const rpc = vi
    .fn()
    .mockResolvedValueOnce({ version: 1, token })
    .mockResolvedValueOnce(prompt(0))
    .mockResolvedValueOnce(prompt(1))
    .mockResolvedValueOnce({ version: 1, state: "connected", session: "7" });
  const result = connectSshRoute(
    nonce,
    chosen,
    new AbortController().signal,
    () => true,
    async () => true,
    rpc as HostRpc,
  );
  chosen.agent = false;
  await expect(result).resolves.toBe("7");
  const sent = rpc.mock.calls[0]?.[1]?.selection;
  expect(sent).toEqual({ ...selection, agent: true });
  expect(Object.isFrozen(sent)).toBe(true);
});
it("rejects ambiguous Agent/private-key selection before issuing a route action", async () => {
  const rpc = vi.fn();
  await expect(
    connectSshRoute(
      nonce,
      { ...selection, agent: true, privateKey: true },
      new AbortController().signal,
      () => true,
      async () => true,
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_denied");
  expect(rpc).not.toHaveBeenCalled();
});

it("continues an original backend CLI ticket without issuing a new GUI begin", async () => {
  let index = 0;
  const rpc = vi.fn(async (command: string) => {
    if (command.endsWith("wait")) return prompt(0);
    if (command.endsWith("confirm"))
      return ++index === 1
        ? prompt(1)
        : { state: "connected", version: 1, session: "42" };
    throw Error("unexpected re-begin");
  });
  expect(
    await continueSshRoute(
      nonce,
      token,
      new AbortController().signal,
      () => true,
      async () => true,
      rpc as HostRpc,
    ),
  ).toBe("42");
  expect(rpc.mock.calls.map(([command]) => command)).toEqual([
    "workspace_ssh_route_wait",
    "workspace_ssh_route_confirm",
    "workspace_ssh_route_confirm",
  ]);
});

it("revokes exactly the inherited CLI route token after a declined actual peer", async () => {
  const rpc = vi.fn(async (command: string) =>
    command.endsWith("wait") ? prompt(0) : null,
  );
  await expect(
    continueSshRoute(
      nonce,
      token,
      new AbortController().signal,
      () => true,
      async () => false,
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_cancelled");
  expect(rpc.mock.calls.map(([command]) => command)).toEqual([
    "workspace_ssh_route_wait",
    "workspace_ssh_route_cancel",
  ]);
  expect(rpc.mock.calls[1]).toEqual([
    "workspace_ssh_route_cancel",
    { nonce, token },
  ]);
});

it("rejects malformed inherited tokens before RPC and cancels when the original page retires", async () => {
  const rpc = vi.fn(async (_command: string, _args?: Record<string, unknown>) =>
    prompt(0),
  );
  await expect(
    continueSshRoute(
      nonce,
      "invalid",
      new AbortController().signal,
      () => true,
      async () => true,
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_denied");
  expect(rpc).not.toHaveBeenCalled();
  let current = true;
  await expect(
    continueSshRoute(
      nonce,
      token,
      new AbortController().signal,
      () => current,
      async () => {
        current = false;
        return true;
      },
      rpc as HostRpc,
    ),
  ).rejects.toThrow("ssh.route_cancelled");
  expect(rpc.mock.calls.some(([command]) => command.endsWith("confirm"))).toBe(
    false,
  );
});
