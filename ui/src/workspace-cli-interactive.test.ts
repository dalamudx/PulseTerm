import { afterEach, expect, it, vi } from "vitest";
import { CliNewTabController, decodeCliNewTab } from "./workspace-cli-new-tab";

it.each([
  "key-interactive",
  "agent-interactive",
  "password-interactive",
] as const)(
  "keeps %s explicit on cold and warm approvals",
  (authentication) => {
    const selected = { ...ssh, authentication: authentication };
    expect(
      decodeCliNewTab({ ...request, ssh: selected }).ssh?.authentication,
    ).toBe(authentication);
    expect(
      decodeCliNewTab({
        version: 3,
        ticket,
        intent: {
          hostId: ssh.hostId,
          identityId: ssh.identityId,
          authentication: authentication,
        },
      }).intent,
    ).toEqual({
      hostId: ssh.hostId,
      identityId: ssh.identityId,
      authentication: authentication,
    });
    for (const intent of [
      { alias: "Saved", authentication: authentication },
      {
        hostId: ssh.hostId,
        identityId: ssh.identityId,
        authentication: authentication,
        answers: [],
      },
      { hostId: ssh.hostId, identityId: ssh.identityId, authentication: null },
    ])
      expect(() => decodeCliNewTab({ version: 3, ticket, intent })).toThrow();
  },
);
it.each([
  "key-interactive",
  "agent-interactive",
  "password-interactive",
] as const)(
  "answers only the approved %s ticket and clears its one-round buffer",
  async (authentication) => {
    const f = fixture();
    await f.controller.challenge(challenge);
    expect(f.interactive).not.toHaveBeenCalled();
    await f.controller.request({
      ...request,
      ssh: { ...ssh, authentication },
    });
    await f.controller.challenge({ ...challenge, ticket: token });
    expect(f.interactive).not.toHaveBeenCalled();
    await f.controller.challenge(challenge);
    expect(f.respondInteractive).toHaveBeenCalledWith(
      nonce,
      token,
      round,
      f.answers,
    );
    expect(f.answers).toEqual([""]);
  },
);
it.each([
  "key-interactive",
  "agent-interactive",
  "password-interactive",
] as const)(
  "cancels the real %s token when its original page retires before a late answer",
  async (authentication) => {
    const f = fixture();
    let finish!: (answers: string[]) => void;
    f.interactive.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          finish = resolve;
        }),
    );
    await f.controller.request({
      ...request,
      ssh: { ...ssh, authentication },
    });
    const waiting = f.controller.challenge(challenge);
    f.retire();
    f.life.abort();
    finish(f.answers);
    await waiting;
    expect(f.respondInteractive).not.toHaveBeenCalled();
    expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
    expect(f.answers).toEqual([""]);
  },
);
const ticket = "11111111-1111-4111-8111-111111111111";
const nonce = "22222222-2222-4222-8222-222222222222";
const token = "33333333-3333-4333-8333-333333333333";
const round = "44444444-4444-4444-8444-444444444444";
const ssh = {
  hostId: "11111111-1111-7111-8111-111111111111",
  identityId: "22222222-2222-7222-8222-222222222222",
  hostLabel: "Saved",
  identityLabel: "Account",
  address: "127.0.0.1",
  port: 2222,
  username: "literal",
  authentication: "interactive" as const,
};
const request = { version: 2, ticket, ssh };
const prompt = {
  version: 1,
  round,
  name: "Server <script>",
  instructions: "literal",
  prompts: [{ text: "Code", echo: false }],
};
const challenge = { version: 1, ticket, token, prompt };
const lives: AbortController[] = [];
afterEach(() => {
  for (const life of lives.splice(0)) life.abort();
});
function fixture() {
  const life = new AbortController();
  lives.push(life);
  let current: string | undefined = nonce;
  const answers = ["123456"];
  const interactive = vi.fn(async () => answers as string[] | undefined);
  const respondInteractive = vi.fn(
    async (..._args: [string, string, string, string[]]) => {},
  );
  const cancelInteractive = vi.fn(async () => {}),
    failed = vi.fn(),
    opened = vi.fn(async () => {});
  const confirmSsh = vi.fn(async () => true),
    answer = vi.fn(async () => {});
  const controller = new CliNewTabController({
    signal: life.signal,
    current: () => current,
    available: () => true,
    confirm: async () => false,
    confirmSsh,
    answer,
    opened,
    failed,
    interactive,
    respondInteractive,
    cancelInteractive,
  });
  return {
    controller,
    life,
    interactive,
    respondInteractive,
    cancelInteractive,
    failed,
    opened,
    answers,
    confirmSsh,
    answer,
    retire: () => {
      current = undefined;
    },
  };
}
it("decodes explicit MFA on cold and warm notices and rejects role mixing", () => {
  expect(decodeCliNewTab(request).ssh?.authentication).toBe("interactive");
  expect(
    decodeCliNewTab({
      version: 3,
      ticket,
      intent: {
        hostId: ssh.hostId,
        identityId: ssh.identityId,
        authentication: "interactive",
      },
    }).intent,
  ).toEqual({
    hostId: ssh.hostId,
    identityId: ssh.identityId,
    authentication: "interactive",
  });
  for (const authentication of [null, false, "password", "auto"])
    expect(() =>
      decodeCliNewTab({ ...request, ssh: { ...ssh, authentication } }),
    ).toThrow();
  for (const flags of [{ privateKey: true }, { agent: true }])
    expect(() =>
      decodeCliNewTab({ ...request, ssh: { ...ssh, ...flags } }),
    ).toThrow();
});
it("does not answer before approval or for another ticket", async () => {
  const f = fixture();
  await f.controller.challenge(challenge);
  expect(f.interactive).not.toHaveBeenCalled();
  await f.controller.request(request);
  await f.controller.challenge({ ...challenge, ticket: token });
  expect(f.interactive).not.toHaveBeenCalled();
  await f.controller.challenge(challenge);
  expect(f.respondInteractive).toHaveBeenCalledWith(
    nonce,
    token,
    round,
    f.answers,
  );
  expect(f.answers).toEqual([""]);
});
it("never upgrades a default password request to MFA", async () => {
  const f = fixture();
  const { authentication: _, ...saved } = ssh;
  await f.controller.request({ ...request, ssh: saved });
  await f.controller.challenge(challenge);
  expect(f.interactive).not.toHaveBeenCalled();
});
it("Esc cancels the native token and does not send any answers", async () => {
  const f = fixture();
  f.interactive.mockResolvedValue(undefined);
  await f.controller.request(request);
  await f.controller.challenge(challenge);
  expect(f.cancelInteractive).toHaveBeenCalledExactlyOnceWith(nonce, token);
  expect(f.respondInteractive).not.toHaveBeenCalled();
  await f.controller.opened({ ticket, session: "1" });
  expect(f.opened).not.toHaveBeenCalled();
});
it("duplicates and substituted native tokens cancel the original request", async () => {
  for (const next of [
    challenge,
    { ...challenge, token: nonce, prompt: { ...prompt, round: ticket } },
  ]) {
    const f = fixture();
    await f.controller.request(request);
    await f.controller.challenge(challenge);
    await f.controller.challenge(next);
    expect(f.respondInteractive).toHaveBeenCalledTimes(1);
    expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
  }
});
it("accepts a new round that arrives before the previous answer IPC resolves", async () => {
  const f = fixture();
  await f.controller.request(request);
  f.respondInteractive.mockImplementationOnce(async () => {
    f.interactive.mockResolvedValueOnce(["next"]);
    await f.controller.challenge({
      ...challenge,
      prompt: { ...prompt, round: nonce },
    });
  });
  await f.controller.challenge(challenge);
  expect(f.respondInteractive).toHaveBeenCalledTimes(2);
  expect(f.failed).not.toHaveBeenCalled();
});
it("page retirement drops late answers, clears their buffer and cancels the original native token", async () => {
  const f = fixture();
  let finish!: (a: string[]) => void;
  f.interactive.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.controller.request(request);
  const waiting = f.controller.challenge(challenge);
  f.retire();
  f.life.abort();
  finish(f.answers);
  await waiting;
  expect(f.respondInteractive).not.toHaveBeenCalled();
  expect(f.answers).toEqual([""]);
  expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
});
it("successful adoption does not cancel the root-owned session", async () => {
  const f = fixture();
  await f.controller.request(request);
  await f.controller.challenge(challenge);
  await f.controller.opened({ ticket, session: "1" });
  f.life.abort();
  expect(f.opened).toHaveBeenCalledExactlyOnceWith(nonce, "1");
  expect(f.cancelInteractive).not.toHaveBeenCalled();
});
it("rejects another simultaneous round while a question is open", async () => {
  const f = fixture();
  let finish!: (a: string[]) => void;
  f.interactive.mockImplementationOnce(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  await f.controller.request(request);
  const waiting = f.controller.challenge(challenge);
  await f.controller.challenge({
    ...challenge,
    prompt: { ...prompt, round: nonce },
  });
  finish(f.answers);
  await waiting;
  expect(f.respondInteractive).not.toHaveBeenCalled();
  expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
});

it("failed answer clears the buffer and revokes the actual request", async () => {
  const f = fixture();
  f.respondInteractive.mockRejectedValueOnce(Error("failed"));
  await f.controller.request(request);
  await f.controller.challenge(challenge);
  expect(f.answers).toEqual([""]);
  expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
  expect(f.failed).toHaveBeenCalledOnce();
});
it("the ninth question round cancels instead of asking or responding", async () => {
  const f = fixture();
  await f.controller.request(request);
  for (let n = 1; n <= 9; n++)
    await f.controller.challenge({
      ...challenge,
      prompt: {
        ...prompt,
        round: `00000000-0000-4000-8000-${String(n).padStart(12, "0")}`,
      },
    });
  expect(f.interactive).toHaveBeenCalledTimes(8);
  expect(f.respondInteractive).toHaveBeenCalledTimes(8);
  expect(f.cancelInteractive).toHaveBeenCalledWith(nonce, token);
});
