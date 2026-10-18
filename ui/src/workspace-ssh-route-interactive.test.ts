import { expect, it, vi } from "vitest";
import type { HostRpc } from "./workspace-hosts";
import { connectSshRoute, type RouteDecision } from "./workspace-ssh-route";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222";
const selection = {
  host: "11111111-1111-7111-8111-111111111111",
  identity: "22222222-2222-7222-8222-222222222222",
  profile: "33333333-3333-7333-8333-333333333333",
  privateKey: false,
};
function hop(index: number, available = true) {
  return {
    state: "confirm",
    version: 1,
    challenge: index ? token : nonce,
    prompt: {
      index,
      count: 2,
      entrance: index ? selection.host : selection.profile,
      label: "literal saved host",
      host: "127.0.0.1",
      port: 22,
      username: "literal account",
      identity: selection.identity,
      identityLabel: "literal identity",
      algorithm: "ssh-ed25519",
      fingerprint: `SHA256:${"A".repeat(43)}`,
      firstUse: false,
      interactiveAvailable: available,
    },
  };
}
function question(round = nonce) {
  return {
    state: "interactive",
    version: 1,
    prompt: {
      version: 1,
      round,
      name: "remote <literal>",
      instructions: "remote instructions",
      prompts: [{ text: "remote code", echo: false }],
    },
  };
}
it("mixes explicitly selected intermediate MFA and a password leaf, clears answers and retains the route token", async () => {
  const answers = ["owned synthetic response"],
    sent: unknown[] = [];
  const ask = vi.fn(async () => answers);
  const rpc = vi.fn(async (command: string, args: unknown) => {
    if (command.endsWith("begin")) return { version: 1, token };
    if (command.endsWith("wait")) return hop(0);
    if (command.endsWith("confirm"))
      return (args as { approved: RouteDecision }).approved === "interactive"
        ? question()
        : { version: 1, state: "connected", session: "7" };
    if (command.endsWith("answer")) {
      sent.push({
        ...(args as object),
        answers: [...(args as { answers: string[] }).answers],
      });
      return hop(1);
    }
    return { version: 1 };
  });
  const confirmed = vi.fn(
    async (p: { index: number }): Promise<RouteDecision> =>
      p.index ? true : "interactive",
  );
  await expect(
    connectSshRoute(
      nonce,
      selection,
      new AbortController().signal,
      () => true,
      confirmed,
      rpc as HostRpc,
      ask,
    ),
  ).resolves.toBe("7");
  expect(ask).toHaveBeenCalledTimes(1);
  expect(answers).toEqual([]);
  expect(sent).toEqual([
    { nonce, token, round: nonce, answers: ["owned synthetic response"] },
  ]);
  expect(rpc.mock.calls.some(([command]) => command.endsWith("cancel"))).toBe(
    false,
  );
});
it.each(["unsolicited", "repeat", "cancel", "unavailable"])(
  "rejects %s MFA without submitting another answer",
  async (variant) => {
    let submitted = 0;
    const rpc = vi.fn(async (command: string) => {
      if (command.endsWith("begin")) return { version: 1, token };
      if (command.endsWith("wait"))
        return variant === "unsolicited"
          ? question()
          : hop(0, variant !== "unavailable");
      if (command.endsWith("confirm")) return question();
      if (command.endsWith("answer")) {
        submitted++;
        return question();
      }
      return { version: 1 };
    });
    const ask = vi.fn(async () =>
      variant === "cancel" ? undefined : ["synthetic"],
    );
    await expect(
      connectSshRoute(
        nonce,
        selection,
        new AbortController().signal,
        () => true,
        async () => "interactive",
        rpc as HostRpc,
        ask,
      ),
    ).rejects.toThrow(/^ssh\.route_/);
    expect(submitted).toBe(variant === "repeat" ? 1 : 0);
    expect(ask).toHaveBeenCalledTimes(
      variant === "cancel" || variant === "repeat" ? 1 : 0,
    );
    expect(
      rpc.mock.calls.filter(([command]) => command.endsWith("cancel")),
    ).toHaveLength(1);
  },
);

type JointMethod =
  | "key-interactive"
  | "agent-interactive"
  | "password-interactive";
function jointHop(index = 0, method: JointMethod = "key-interactive") {
  const reply = hop(index, false);
  return {
    ...reply,
    prompt: {
      ...reply.prompt,
      ...(method === "password-interactive"
        ? { interactiveAvailable: true }
        : {}),
      [method === "password-interactive"
        ? "passwordInteractiveAvailable"
        : method === "agent-interactive"
          ? "agentInteractiveAvailable"
          : "keyInteractiveAvailable"]: true,
    },
  };
}
function jointSelection(method: JointMethod) {
  return method === "password-interactive"
    ? selection
    : method === "agent-interactive"
      ? { ...selection, agent: true }
      : { ...selection, privateKey: true };
}
it.each([
  "key-interactive",
  "agent-interactive",
  "password-interactive",
] as const)(
  "mixes explicit %s with a saved leaf, uses the original token and clears answers",
  async (method) => {
    const answers = ["synthetic joint answer"],
      sent: unknown[] = [];
    let hopIndex = 0;
    const rpc = vi.fn(async (command: string, args: unknown) => {
      if (command.endsWith("begin")) return { version: 1, token };
      if (command.endsWith("wait")) return jointHop(0, method);
      if (command.endsWith("confirm")) {
        expect((args as { approved: RouteDecision }).approved).toBe(
          hopIndex ? true : method,
        );
        return hopIndex
          ? { version: 1, state: "connected", session: "9" }
          : question();
      }
      if (command.endsWith("answer")) {
        sent.push({
          ...(args as object),
          answers: [...(args as { answers: string[] }).answers],
        });
        hopIndex = 1;
        return jointHop(1, method);
      }
      return { version: 1 };
    });
    await expect(
      connectSshRoute(
        nonce,
        jointSelection(method),
        new AbortController().signal,
        () => true,
        async (p) => (p.index ? true : method),
        rpc as HostRpc,
        async () => answers,
      ),
    ).resolves.toBe("9");
    expect(sent).toEqual([
      { nonce, token, round: nonce, answers: ["synthetic joint answer"] },
    ]);
    expect(answers).toEqual([]);
    expect(rpc.mock.calls.some(([command]) => command.endsWith("cancel"))).toBe(
      false,
    );
  },
);
it.each(
  (
    ["key-interactive", "agent-interactive", "password-interactive"] as const
  ).flatMap((method) =>
    [
      "unavailable",
      "wrong-kind",
      "repeat",
      "cancel",
      "unsolicited",
      "bad-capability",
      "both-kinds",
      "both-keys",
    ].map((variant) => ({ method, variant })),
  ),
)(
  "rejects $variant $method without another answer or authentication fallback",
  async ({ method, variant }) => {
    let submitted = 0;
    const answers = ["synthetic joint answer"];
    const rpc = vi.fn(async (command: string) => {
      if (command.endsWith("begin")) return { version: 1, token };
      if (command.endsWith("wait")) {
        if (variant === "unsolicited") return question();
        if (variant === "unavailable") return hop(0, false);
        if (variant === "wrong-kind") return hop(0, true);
        const reply = jointHop(0, method);
        if (variant === "bad-capability")
          return {
            ...reply,
            prompt: {
              ...reply.prompt,
              [method === "password-interactive"
                ? "passwordInteractiveAvailable"
                : method === "agent-interactive"
                  ? "agentInteractiveAvailable"
                  : "keyInteractiveAvailable"]: "yes",
            },
          };
        if (variant === "both-kinds")
          return {
            ...reply,
            prompt: {
              ...reply.prompt,
              ...(method === "password-interactive"
                ? { keyInteractiveAvailable: true }
                : { interactiveAvailable: true }),
            },
          };
        if (variant === "both-keys")
          return {
            ...reply,
            prompt: {
              ...reply.prompt,
              keyInteractiveAvailable: true,
              agentInteractiveAvailable: true,
            },
          };
        return reply;
      }
      if (command.endsWith("confirm")) return question();
      if (command.endsWith("answer")) {
        submitted++;
        return question();
      }
      return { version: 1 };
    });
    const ask = vi.fn(async () => (variant === "cancel" ? undefined : answers));
    await expect(
      connectSshRoute(
        nonce,
        jointSelection(method),
        new AbortController().signal,
        () => true,
        async () => method,
        rpc as HostRpc,
        ask,
      ),
    ).rejects.toThrow(/^ssh\.route_/);
    expect(submitted).toBe(variant === "repeat" ? 1 : 0);
    expect(ask).toHaveBeenCalledTimes(
      ["repeat", "cancel"].includes(variant) ? 1 : 0,
    );
    if (variant === "repeat") expect(answers).toEqual([]);
    expect(
      rpc.mock.calls.filter(([command]) => command.endsWith("cancel")),
    ).toHaveLength(1);
  },
);
