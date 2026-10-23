import { afterEach, expect, it, vi } from "vitest";
import { CliNewTabController, decodeCliNewTab } from "./workspace-cli-new-tab";

it("accepts only canonical saved identities in normal and cold approvals", () => {
  const profile = "019a0000-0000-7000-8000-000000000001";
  expect(decodeCliNewTab({ version: 1, ticket, profile }).profile).toBe(
    profile,
  );
  expect(
    decodeCliNewTab({
      version: 4,
      stage: "base",
      direction: "row",
      ticket,
      profile,
    }).profile,
  ).toBe(profile);
  for (const invalid of [
    "/bin/sh",
    "../sh",
    profile.toUpperCase(),
    "11111111-1111-4111-8111-111111111111",
  ]) {
    expect(() =>
      decodeCliNewTab({ version: 1, ticket, profile: invalid }),
    ).toThrow();
  }
});

const ticket = "11111111-1111-4111-8111-111111111111";
const nonce = "22222222-2222-4222-8222-222222222222";
const request = { version: 1, ticket, profile: "sh" };
afterEach(() => vi.useRealTimers());
function fixture() {
  const life = new AbortController();
  let current: string | undefined = nonce;
  let available = true;
  let splitTarget: { session: string; identity: object } | undefined = {
    session: "1",
    identity: {},
  };
  let decide: (accept: boolean) => void = () => {};
  let decisionSignal: AbortSignal | undefined;
  const confirm = vi.fn((_profile: string, signal: AbortSignal) => {
    decisionSignal = signal;
    return new Promise<boolean>((resolve) => {
      decide = resolve;
      signal.addEventListener("abort", () => resolve(false), { once: true });
    });
  });
  const confirmWindow = vi.fn((signal: AbortSignal) => {
    decisionSignal = signal;
    return new Promise<boolean>((resolve) => {
      decide = resolve;
      signal.addEventListener("abort", () => resolve(false), { once: true });
    });
  });
  const answer = vi.fn(async () => {}),
    opened = vi.fn(async () => {}),
    failed = vi.fn(),
    admitPresentation = vi.fn(async () => {}),
    completePresentation = vi.fn(async () => {});
  const controller = new CliNewTabController({
    signal: life.signal,
    current: () => current,
    available: () => available,
    captureSplit: () => splitTarget,
    admitPresentation,
    completePresentation,
    confirm,
    confirmWindow,
    answer,
    opened,
    failed,
  });
  return {
    controller,
    life,
    confirm,
    confirmWindow,
    answer,
    opened,
    failed,
    admitPresentation,
    completePresentation,
    decide: (accept: boolean) => decide(accept),
    current: (value: string | undefined) => {
      current = value;
    },
    available: (value: boolean) => {
      available = value;
    },
    signal: () => decisionSignal,
    target: (value: { session: string; identity: object } | undefined) => {
      splitTarget = value;
    },
  };
}
it("window approval has a closed distinct notice and never accepts a Profile or target", () => {
  expect(decodeCliNewTab({ version: 5, ticket })).toEqual({
    ticket,
    newWindow: true,
  });
  for (const value of [
    { version: 5, ticket, profile: "sh" },
    { version: 5, ticket, window: "workspace" },
    { version: 5, ticket: "invalid" },
  ])
    expect(() => decodeCliNewTab(value)).toThrow();
});
it("accepting an empty window clears its approval without waiting for a session or invoking Profile confirmation", async () => {
  const f = fixture();
  const job = f.controller.request({ version: 5, ticket });
  expect(f.confirmWindow).toHaveBeenCalledTimes(1);
  expect(f.confirm).not.toHaveBeenCalled();
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, true);
  expect(f.opened).not.toHaveBeenCalled();
  expect(f.admitPresentation).not.toHaveBeenCalled();
  const next = f.controller.request(request);
  expect(f.confirm).toHaveBeenCalledTimes(1);
  f.decide(false);
  await next;
  f.life.abort();
});
it("cancelling the empty window never calls opened", async () => {
  const f = fixture();
  const job = f.controller.request({ version: 5, ticket });
  f.decide(false);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
it("an old page cannot approve its pending new window", async () => {
  const f = fixture();
  const job = f.controller.request({ version: 5, ticket });
  f.current(undefined);
  f.decide(true);
  await job;
  expect(f.answer).not.toHaveBeenCalled();
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
const presentation = "33333333-3333-4333-8333-333333333333";
const presented = { version: 5, ticket, nonce, session: "2", presentation };
async function acceptedFixture() {
  const f = fixture();
  const job = f.controller.request(request);
  f.decide(true);
  await job;
  return f;
}
it("local presentation admits once and awaits input/layout completion before ACK", async () => {
  const f = await acceptedFixture();
  let resolve: () => void = () => {};
  f.opened.mockImplementation(
    () =>
      new Promise<void>((r) => {
        resolve = r;
      }),
  );
  const job = f.controller.opened(presented);
  await vi.waitFor(() => expect(f.opened).toHaveBeenCalledTimes(1));
  await f.controller.opened(presented);
  expect(f.admitPresentation).toHaveBeenCalledExactlyOnceWith(
    nonce,
    presentation,
    "2",
  );
  expect(f.completePresentation).not.toHaveBeenCalled();
  resolve();
  await job;
  expect(f.completePresentation).toHaveBeenCalledExactlyOnceWith(
    nonce,
    presentation,
    "2",
    true,
  );
});
it("local presentation rejects mismatched nonce, extra fields and malformed tickets before admission", async () => {
  const f = await acceptedFixture();
  for (const body of [
    { ...presented, nonce: ticket },
    { ...presented, presentation: "bad" },
    { ...presented, extra: true },
    { ...presented, version: 6 },
  ])
    await f.controller.opened(body);
  expect(f.admitPresentation).not.toHaveBeenCalled();
  expect(f.opened).not.toHaveBeenCalled();
});
it("rejected local presentation admission cannot show the registered session", async () => {
  const f = await acceptedFixture();
  f.admitPresentation.mockRejectedValue(Error("revoked"));
  await f.controller.opened(presented);
  expect(f.opened).not.toHaveBeenCalled();
  expect(f.completePresentation).not.toHaveBeenCalled();
  expect(f.failed).toHaveBeenCalledTimes(1);
});
it("layout or input failure supplies false completion once", async () => {
  const f = await acceptedFixture();
  const failure = Error("input.uncertain");
  f.opened.mockRejectedValue(failure);
  await f.controller.opened(presented);
  expect(f.completePresentation).toHaveBeenCalledExactlyOnceWith(
    nonce,
    presentation,
    "2",
    false,
  );
  expect(f.failed).toHaveBeenCalledExactlyOnceWith(failure);
});
it("navigation while showing cannot publish the old-page ACK", async () => {
  const f = await acceptedFixture();
  f.opened.mockImplementation(async () => {
    f.current(ticket);
  });
  await f.controller.opened(presented);
  expect(f.completePresentation).not.toHaveBeenCalled();
});
it("uncertain local completion is never retried as false or another success", async () => {
  const f = await acceptedFixture();
  f.completePresentation.mockRejectedValue(Error("reply lost"));
  await f.controller.opened(presented);
  expect(f.completePresentation).toHaveBeenCalledExactlyOnceWith(
    nonce,
    presentation,
    "2",
    true,
  );
  expect(f.failed).toHaveBeenCalledTimes(1);
});
it("pane replacement during native admission rejects before layout effects", async () => {
  const f = fixture();
  const job = f.controller.request({ ...request, direction: "row" });
  f.decide(true);
  await job;
  f.admitPresentation.mockImplementation(async () => {
    f.target({ session: "1", identity: {} });
  });
  await f.controller.opened(presented);
  expect(f.opened).not.toHaveBeenCalled();
  expect(f.completePresentation).toHaveBeenCalledExactlyOnceWith(
    nonce,
    presentation,
    "2",
    false,
  );
});
it("saved alias cold intent rejects mixed IDs, extra execution fields and noncanonical names", () => {
  const payload = { version: 3, ticket, intent: { alias: "生产入口 Alpha" } };
  expect(decodeCliNewTab(payload)).toEqual({ ticket, intent: payload.intent });
  for (const intent of [
    { alias: "x", hostId: "018f5691-0059-7a69-a45a-7c4d6a1cfa51" },
    { alias: "x", program: "/bin/sh" },
    { alias: "" },
    { alias: " x" },
    { alias: "x\n" },
    { alias: "x\u200b" },
    { alias: "x".repeat(257) },
    { alias: "字".repeat(86) },
  ]) {
    expect(() => decodeCliNewTab({ ...payload, intent })).toThrow();
  }
});
it("rejects executable, argv, wrong-version and malformed UUID before asking GUI", () => {
  for (const value of [
    null,
    [],
    { ...request, version: 2 },
    { ...request, ticket: "old" },
    { ...request, profile: "/bin/sh" },
    { ...request, program: "sh" },
    { ...request, env: {} },
  ]) {
    expect(() => decodeCliNewTab(value)).toThrow();
  }
  expect(decodeCliNewTab(request)).toEqual({ ticket, profile: "sh" });
  for (const direction of [null, "-v", "diagonal", 1, {}]) {
    expect(() => decodeCliNewTab({ ...request, direction })).toThrow();
  }
  expect(decodeCliNewTab({ ...request, direction: "row" }).direction).toBe(
    "row",
  );
});
it("explicit approval sends exact nonce/ticket once and never starts before native receipt", async () => {
  const f = fixture();
  const job = f.controller.request(request);
  await f.controller.request(request);
  expect(f.confirm).toHaveBeenCalledTimes(1);
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, true);
  expect(f.opened).not.toHaveBeenCalled();
  await f.controller.opened({
    ticket: "33333333-3333-4333-8333-333333333333",
    session: "2",
  });
  await f.controller.opened({ ticket, session: "bad" });
  expect(f.opened).not.toHaveBeenCalled();
  await f.controller.opened({ ticket, session: "2" });
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).toHaveBeenCalledExactlyOnceWith(nonce, "2");
});
it("cancel and modal/busy rejection do not grant start", async () => {
  const f = fixture();
  const job = f.controller.request(request);
  f.decide(false);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).not.toHaveBeenCalled();
  const g = fixture();
  g.available(false);
  await g.controller.request(request);
  expect(g.confirm).not.toHaveBeenCalled();
  expect(g.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
});
it("page change, lifetime abort and native cancellation defeat a late click", async () => {
  for (const mode of ["page", "abort", "cancel"]) {
    const f = fixture();
    const job = f.controller.request(request);
    if (mode === "page") f.current(undefined);
    else if (mode === "abort") f.life.abort();
    else f.controller.cancel({ ticket });
    f.decide(true);
    await job;
    expect(f.answer).not.toHaveBeenCalled();
    await f.controller.opened({ ticket, session: "2" });
    expect(f.opened).not.toHaveBeenCalled();
  }
});
it("old cancellation does not close a newer request; finite expiry cancels current modal", async () => {
  vi.useFakeTimers();
  const f = fixture();
  const first = f.controller.request(request);
  f.decide(false);
  await first;
  const nextTicket = "33333333-3333-4333-8333-333333333333";
  const next = f.controller.request({ ...request, ticket: nextTicket });
  f.controller.cancel({ ticket });
  expect(f.signal()?.aborted).toBe(false);
  await vi.advanceTimersByTimeAsync(35_000);
  await next;
  expect(f.signal()?.aborted).toBe(true);
  expect(f.answer).toHaveBeenCalledTimes(1);
});
it("lost answer response never retries and native receipt cannot repair unknown authorization", async () => {
  const f = fixture();
  f.answer.mockRejectedValueOnce(new Error("uncertain"));
  const job = f.controller.request(request);
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledTimes(1);
  expect(f.failed).toHaveBeenCalledTimes(1);
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).not.toHaveBeenCalled();
});
it("receipt reentrancy before IPC answer completes is safe and current-page-bound", async () => {
  const f = fixture();
  let done: () => void = () => {};
  f.answer.mockImplementationOnce(
    () =>
      new Promise<void>((resolve) => {
        done = resolve;
      }),
  );
  const job = f.controller.request(request);
  f.decide(true);
  await Promise.resolve();
  await f.controller.opened({ ticket, session: "2" });
  done();
  await job;
  expect(f.opened).toHaveBeenCalledTimes(1);
  const g = fixture();
  const other = g.controller.request(request);
  g.decide(true);
  await other;
  g.current("44444444-4444-4444-8444-444444444444");
  await g.controller.opened({ ticket, session: "2" });
  expect(g.opened).not.toHaveBeenCalled();
});
it("availability is rechecked at click and stale lifetime never answers", async () => {
  const f = fixture();
  const job = f.controller.request(request);
  f.available(false);
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  const g = fixture();
  g.life.abort();
  await g.controller.request(request);
  expect(g.confirm).not.toHaveBeenCalled();
});

it.each(["row", "column"])(
  "split %s captures original pane and only mounts distinct new session",
  async (direction) => {
    const f = fixture();
    const original = { session: "1", identity: {} };
    f.target(original);
    const job = f.controller.request({ ...request, direction });
    f.decide(true);
    await job;
    await f.controller.opened({ ticket, session: "2" });
    expect(f.opened).toHaveBeenCalledExactlyOnceWith(nonce, "2", {
      direction,
      target: original,
    });
  },
);
it("split without a ready target layout rejects before GUI approval", async () => {
  const f = fixture();
  f.target(undefined);
  await f.controller.request({ ...request, direction: "row" });
  expect(f.confirm).not.toHaveBeenCalled();
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
});
it("same session remounted during approval cannot inherit old split intent", async () => {
  const f = fixture();
  const job = f.controller.request({ ...request, direction: "row" });
  f.target({ session: "1", identity: {} });
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
});
it.each(["remount", "ended", "same-session"])(
  "post-start %s refuses split without rolling back root",
  async (mode) => {
    const f = fixture();
    const job = f.controller.request({ ...request, direction: "row" });
    f.decide(true);
    await job;
    if (mode === "remount") f.target({ session: "1", identity: {} });
    if (mode === "ended") f.target(undefined);
    await f.controller.opened({
      ticket,
      session: mode === "same-session" ? "1" : "2",
    });
    expect(f.opened).not.toHaveBeenCalled();
    expect(f.failed).toHaveBeenCalledTimes(1);
    expect(f.answer).toHaveBeenCalledTimes(1);
  },
);

it("cold metadata releases its pending before exact target approval and never accepts addresses", async () => {
  const life = new AbortController();
  const intent = {
    hostId: "018f5691-0059-7a69-a45a-7c4d6a1cfa51",
    identityId: "018f5691-0059-7a69-a45a-7c4d6a1cfa52",
  };
  const cold = { version: 3, ticket, intent };
  expect(decodeCliNewTab(cold)).toEqual({ ticket, intent });
  for (const changed of [
    { ...cold, address: "localhost" },
    { ...cold, intent: { ...intent, username: "root" } },
    { ...cold, intent: { ...intent, hostId: ticket } },
  ]) {
    expect(() => decodeCliNewTab(changed)).toThrow();
  }
  const confirmColdSsh = vi.fn(async () => true),
    confirmSsh = vi.fn(async () => false);
  let controller: CliNewTabController;
  const answer = vi.fn(
    async (_nonce: string, value: string, _accept: boolean) => {
      if (value === ticket)
        await controller.request({
          version: 2,
          ticket: "33333333-3333-4333-8333-333333333333",
          ssh: {
            ...intent,
            hostLabel: "Host",
            address: "localhost",
            port: 22,
            identityLabel: "Identity",
            username: "user",
          },
        });
    },
  );
  controller = new CliNewTabController({
    signal: life.signal,
    current: () => nonce,
    available: () => true,
    confirm: vi.fn(),
    confirmColdSsh,
    confirmSsh,
    answer,
    opened: vi.fn(),
    failed: vi.fn(),
  });
  await controller.request(cold);
  expect(confirmColdSsh).toHaveBeenCalledExactlyOnceWith(
    intent,
    expect.any(AbortSignal),
  );
  expect(confirmSsh).toHaveBeenCalledTimes(1);
  expect(answer.mock.calls.map((args) => args.slice(1))).toEqual([
    [ticket, true],
    ["33333333-3333-4333-8333-333333333333", false],
  ]);
  life.abort();
});

const coldBase = {
  version: 4,
  stage: "base",
  ticket,
  profile: "sh",
  direction: "row",
};
const coldSplit = { ...coldBase, stage: "split", target: "1" };
it("cold split wire requires closed stages and a canonical explicit target", () => {
  expect(decodeCliNewTab(coldBase)).toEqual({
    ticket,
    profile: "sh",
    baseDirection: "row",
  });
  expect(decodeCliNewTab(coldSplit)).toEqual({
    ticket,
    profile: "sh",
    direction: "row",
    targetSession: "1",
  });
  for (const bad of [
    { ...coldBase, target: "1" },
    { ...coldSplit, target: "01" },
    { ...coldSplit, target: 1 },
    { ...coldBase, stage: "auto" },
    { ...coldSplit, program: "sh" },
    { ...coldBase, direction: "diagonal" },
  ]) {
    expect(() => decodeCliNewTab(bad)).toThrow();
  }
});
it("cold base asks independently with no existing pane, then cancellation starts nothing", async () => {
  const f = fixture();
  f.target(undefined);
  const job = f.controller.request(coldBase);
  expect(f.confirm).toHaveBeenCalledExactlyOnceWith(
    "sh",
    expect.any(AbortSignal),
    undefined,
    "row",
  );
  f.decide(false);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  await f.controller.opened({ ticket, session: "1" });
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
it("cold followup rejects a different active session before approval", async () => {
  const f = fixture();
  f.target({ session: "2", identity: {} });
  await f.controller.request(coldSplit);
  expect(f.confirm).not.toHaveBeenCalled();
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  f.life.abort();
});
it("cold followup freezes pane identity through the independent confirmation", async () => {
  const f = fixture();
  const job = f.controller.request(coldSplit);
  f.target({ session: "1", identity: {} });
  f.decide(true);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  await f.controller.opened({ ticket, session: "3" });
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});

it("directory requests preserve exact Unicode and reject extra authority or hidden text", () => {
  const value = {
    version: 6,
    ticket,
    profile: "sh",
    cwd: "/项目/space folder",
  };
  expect(decodeCliNewTab(value)).toEqual({
    ticket,
    profile: "sh",
    cwd: value.cwd,
  });
  for (const extra of [
    { cwd: "" },
    { cwd: null },
    { cwd: "/bad\npath" },
    { cwd: "/hidden\u202epath" },
    { cwd: `/${"界".repeat(1400)}` },
    { direction: "row" },
    { executable: "sh" },
    { version: 1 },
  ]) {
    expect(() => decodeCliNewTab({ ...value, ...extra })).toThrow();
  }
});
it("directory approval shows the exact path but returns only the original ticket and decision", async () => {
  const f = fixture();
  const cwd = "/项目/space folder";
  const job = f.controller.request({ version: 6, ticket, profile: "sh", cwd });
  expect(f.confirm).toHaveBeenCalledExactlyOnceWith(
    "sh",
    expect.any(AbortSignal),
    undefined,
    undefined,
    cwd,
  );
  f.decide(false);
  await job;
  expect(f.answer).toHaveBeenCalledExactlyOnceWith(nonce, ticket, false);
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
it("navigation cannot transfer a directory approval to the new page", async () => {
  const f = fixture();
  const job = f.controller.request({
    version: 6,
    ticket,
    profile: "sh",
    cwd: "/workspace",
  });
  f.current(undefined);
  f.decide(true);
  await job;
  expect(f.answer).not.toHaveBeenCalled();
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
