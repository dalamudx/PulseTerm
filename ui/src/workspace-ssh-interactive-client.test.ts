import { afterEach, describe, expect, it, vi } from "vitest";
import type { HostRpc } from "./workspace-hosts";

const { channels } = vi.hoisted(() => ({
  channels: [] as Array<{ onmessage: (value: unknown) => Promise<void> }>,
}));
vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(),
  Channel: class {
    onmessage = async (_value: unknown) => {};
    constructor() {
      channels.push(this);
    }
  },
}));

import {
  connectSavedAgentInteractiveSsh,
  connectSavedInteractiveSsh,
  connectSavedPasswordInteractiveSsh,
  connectSavedPrivateKeyInteractiveSsh,
} from "./workspace-ssh-session";

const nonce = "11111111-1111-4111-8111-111111111111",
  token = "22222222-2222-4222-8222-222222222222";
const selection = {
  host: "11111111-1111-7111-8111-111111111111",
  identity: "22222222-2222-7222-8222-222222222222",
};
const packet = {
  version: 1,
  round: "33333333-3333-4333-8333-333333333333",
  name: "保存",
  instructions: "fixture",
  prompts: [{ text: "OTP", echo: false }],
};
function channel() {
  const value = channels[0];
  if (!value) throw Error("fixture.channel");
  return value;
}
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}
function fixture(connectExplicit = connectSavedInteractiveSsh) {
  const start = deferred<unknown>(),
    reached = deferred<void>(),
    controller = new AbortController();
  let current = true;
  const rpc = vi.fn<HostRpc>(async (command) => {
    if (command.endsWith("_begin")) return { version: 1, token };
    if (command === "workspace_ssh_session_start") {
      reached.resolve();
      return start.promise;
    }
    return null;
  });
  const connect = (
    questions: (
      prompt: unknown,
      signal: AbortSignal,
    ) => Promise<string[] | undefined>,
  ) =>
    connectExplicit(
      nonce,
      selection,
      controller.signal,
      () => current,
      questions,
      rpc,
    );
  return {
    start,
    reached,
    controller,
    rpc,
    connect,
    stale: () => {
      current = false;
    },
  };
}
afterEach(() => {
  channels.length = 0;
  vi.restoreAllMocks();
  vi.useRealTimers();
});
describe.each([
  {
    kind: "metadata MFA",
    connect: connectSavedInteractiveSsh,
    begin: "workspace_ssh_interactive_session_begin",
  },
  {
    kind: "private key plus MFA",
    connect: connectSavedPrivateKeyInteractiveSsh,
    begin: "workspace_ssh_private_key_interactive_session_begin",
  },
  {
    kind: "password plus MFA",
    connect: connectSavedPasswordInteractiveSsh,
    begin: "workspace_ssh_password_interactive_session_begin",
  },
  {
    kind: "Agent signature plus MFA",
    connect: connectSavedAgentInteractiveSsh,
    begin: "workspace_ssh_agent_interactive_session_begin",
  },
])("explicit $kind authentication", ({ connect, begin }) => {
  it("waits for a submitted answer receipt before presenting an already received next round", async () => {
    const f = fixture(connect),
      receipt = deferred<unknown>(),
      original = f.rpc.getMockImplementation()!;
    f.rpc.mockImplementation(async (command, args) =>
      command.endsWith("_answer") ? receipt.promise : original(command, args),
    );
    const questions = vi.fn(async () => ["fixture"]),
      result = f.connect(questions).catch((error) => error);
    await f.reached.promise;
    const first = channel().onmessage(packet);
    await vi.waitFor(() =>
      expect(
        f.rpc.mock.calls.some(([command]) => command.endsWith("_answer")),
      ).toBe(true),
    );
    const next = channel().onmessage({
      ...packet,
      round: "44444444-4444-4444-8444-444444444444",
    });
    expect(questions).toHaveBeenCalledOnce();
    expect(
      f.rpc.mock.calls.some(([command]) => command.endsWith("_cancel")),
    ).toBe(false);
    receipt.resolve(null);
    await Promise.all([first, next]);
    expect(questions).toHaveBeenCalledTimes(2);
    f.start.resolve("9");
    expect(await result).toBe("9");
  });
  it.each(["invalid receipt", "cancelled page", "extra queued round"])(
    "never presents a queued round after %s",
    async (failure) => {
      const f = fixture(connect),
        receipt = deferred<unknown>(),
        original = f.rpc.getMockImplementation()!;
      f.rpc.mockImplementation(async (command, args) =>
        command.endsWith("_answer") ? receipt.promise : original(command, args),
      );
      const questions = vi.fn(async () => ["fixture"]),
        result = f.connect(questions).catch((error) => error);
      await f.reached.promise;
      const first = channel().onmessage(packet);
      await vi.waitFor(() =>
        expect(
          f.rpc.mock.calls.some(([command]) => command.endsWith("_answer")),
        ).toBe(true),
      );
      const next = channel().onmessage({
        ...packet,
        round: "44444444-4444-4444-8444-444444444444",
      });
      if (failure === "cancelled page") f.controller.abort();
      if (failure === "extra queued round")
        await channel().onmessage({
          ...packet,
          round: "55555555-5555-4555-8555-555555555555",
        });
      receipt.resolve(
        failure === "invalid receipt" ? { unexpected: true } : null,
      );
      await Promise.all([first, next]);
      expect(await result).toMatchObject({ message: "ssh.session_cancelled" });
      expect(questions).toHaveBeenCalledOnce();
      expect(
        f.rpc.mock.calls.filter(([command]) => command.endsWith("_cancel")),
      ).toHaveLength(1);
    },
  );
  it("answers only the original token and round, clears application arrays and decodes numeric session IDs", async () => {
    const f = fixture(connect),
      answers = ["fixture-answer"],
      questions = vi.fn(async () => answers);
    const pending = f.connect(questions);
    await f.reached.promise;
    expect(f.rpc).toHaveBeenCalledWith(
      begin,
      expect.objectContaining({ nonce, selection, challenges: channels[0] }),
    );
    await channel().onmessage(packet);
    expect(f.rpc).toHaveBeenCalledWith(
      "workspace_ssh_interactive_session_answer",
      expect.objectContaining({ nonce, token, round: packet.round }),
    );
    expect(answers).toEqual([""]);
    expect(questions).toHaveBeenCalledOnce();
    f.start.resolve("7");
    await expect(pending).resolves.toBe("7");
    await channel().onmessage(packet);
    expect(questions).toHaveBeenCalledOnce();
    expect(
      f.rpc.mock.calls.some(
        ([command]) =>
          command === "workspace_ssh_session_begin" ||
          command === "workspace_ssh_key_session_begin",
      ),
    ).toBe(false);
  });
  it("cancels pending questions and never posts answers after the original page becomes stale", async () => {
    const f = fixture(connect),
      answer = deferred<string[] | undefined>(),
      questions = vi.fn(() => answer.promise);
    const pending = f.connect(questions);
    await f.reached.promise;
    const response = channel().onmessage(packet);
    f.stale();
    answer.resolve(["fixture-answer"]);
    await response;
    await expect(pending).rejects.toThrow("ssh.session_cancelled");
    expect(
      f.rpc.mock.calls.filter(([command]) => command.endsWith("_cancel")),
    ).toHaveLength(1);
    expect(
      f.rpc.mock.calls.some(([command]) => command.endsWith("_answer")),
    ).toBe(false);
  });
  it("aborts the question signal on explicit cancellation and ignores late answers", async () => {
    const f = fixture(connect),
      answer = deferred<string[] | undefined>();
    let questionSignal: AbortSignal | undefined;
    const pending = f.connect(async (_prompt, signal) => {
      questionSignal = signal;
      return answer.promise;
    });
    await f.reached.promise;
    const response = channel().onmessage(packet);
    const rejected = expect(pending).rejects.toThrow("ssh.session_cancelled");
    f.controller.abort();
    await rejected;
    expect(questionSignal?.aborted).toBe(true);
    answer.resolve(["fixture-answer"]);
    await response;
    expect(
      f.rpc.mock.calls.some(([command]) => command.endsWith("_answer")),
    ).toBe(false);
  });
  it("rejects malformed server questions before showing inputs", async () => {
    const f = fixture(connect),
      questions = vi.fn(async () => ["fixture"]);
    const pending = f.connect(questions);
    await f.reached.promise;
    await channel().onmessage({
      ...packet,
      prompts: Array(9).fill(packet.prompts[0]),
    });
    await expect(pending).rejects.toThrow("ssh.session_cancelled");
    expect(questions).not.toHaveBeenCalled();
  });
  it("keeps one fixed five-minute deadline for all rounds", async () => {
    vi.useFakeTimers();
    const f = fixture(connect),
      pending = f.connect(async () => ["fixture"]);
    await f.reached.promise;
    const rejected = expect(pending).rejects.toThrow("ssh.session_uncertain");
    await vi.advanceTimersByTimeAsync(300_000);
    await rejected;
    expect(
      f.rpc.mock.calls.filter(([command]) => command.endsWith("_cancel")),
    ).toHaveLength(1);
  });
  it("does not present the same round twice or overlap questions", async () => {
    const f = fixture(connect),
      answer = deferred<string[] | undefined>(),
      questions = vi.fn(() => answer.promise);
    const pending = f.connect(questions);
    await f.reached.promise;
    const first = channel().onmessage(packet);
    await channel().onmessage(packet);
    await expect(pending).rejects.toThrow("ssh.session_cancelled");
    expect(questions).toHaveBeenCalledOnce();
    answer.resolve(["fixture"]);
    await first;
    expect(
      f.rpc.mock.calls.some(([command]) => command.endsWith("_answer")),
    ).toBe(false);
  });
});
