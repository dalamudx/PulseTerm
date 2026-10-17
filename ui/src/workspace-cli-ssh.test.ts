import { expect, it, vi } from "vitest";
import { CliNewTabController, decodeCliNewTab } from "./workspace-cli-new-tab";

const ticket = "11111111-1111-4111-8111-111111111111";
const nonce = "22222222-2222-4222-8222-222222222222";
const ssh = {
  hostId: "11111111-1111-7111-8111-111111111111",
  identityId: "22222222-2222-7222-8222-222222222222",
  hostLabel: "Saved SSH",
  address: "127.0.0.1",
  port: 2222,
  identityLabel: "Saved identity",
  username: "fixture",
};
const request = { version: 2, ticket, ssh };
function fixture(answer = true) {
  const life = new AbortController();
  let current: string | undefined = nonce;
  const confirm = vi.fn(async () => true);
  const confirmSsh = vi.fn(async () => answer);
  const answerFn = vi.fn(async () => {}),
    opened = vi.fn(async () => {}),
    failed = vi.fn();
  const controller = new CliNewTabController({
    signal: life.signal,
    current: () => current,
    available: () => true,
    confirm,
    confirmSsh,
    answer: answerFn,
    opened,
    failed,
  });
  return {
    life,
    controller,
    confirm,
    confirmSsh,
    answer: answerFn,
    opened,
    failed,
    retire: () => {
      current = undefined;
    },
  };
}
it("decodes only bounded saved SSH display metadata", () => {
  expect(decodeCliNewTab(request)).toEqual({ ticket, ssh });
  for (const raw of [
    { ...request, program: "/bin/sh" },
    { ...request, profile: "sh" },
    { ...request, ssh: { ...ssh, password: "secret" } },
    { ...request, ssh: { ...ssh, hostId: ticket } },
    { ...request, ssh: { ...ssh, port: 0 } },
    { ...request, ssh: { ...ssh, port: 1.5 } },
    { ...request, ssh: { ...ssh, port: 65536 } },
    { ...request, ssh: { ...ssh, username: "x\ncommand" } },
    { ...request, ssh: { ...ssh, hostLabel: "x".repeat(513) } },
    { ...request, ssh: { ...ssh, address: null } },
  ])
    expect(() => decodeCliNewTab(raw)).toThrow();
});
it("approved saved SSH never calls local Profile confirmation and accepts only the correlated session", async () => {
  const f = fixture();
  await f.controller.request(request);
  expect(f.confirm).not.toHaveBeenCalled();
  expect(f.confirmSsh).toHaveBeenCalledWith(ssh, expect.any(AbortSignal));
  expect(f.answer).toHaveBeenCalledWith(nonce, ticket, true);
  await f.controller.opened({ ticket: nonce, session: "2" });
  expect(f.opened).not.toHaveBeenCalled();
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).toHaveBeenCalledExactlyOnceWith(nonce, "2");
  f.life.abort();
});
it("SSH Cancel grants no display/input effect", async () => {
  const f = fixture(false);
  await f.controller.request(request);
  expect(f.answer).toHaveBeenCalledWith(nonce, ticket, false);
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
it("retired SSH approval and session receipts cannot act on the replacement page", async () => {
  const f = fixture();
  await f.controller.request(request);
  f.retire();
  f.life.abort();
  await f.controller.opened({ ticket, session: "2" });
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
it("page change while deciding cannot approve or display", async () => {
  const f = fixture();
  let finish: (v: boolean) => void = () => {};
  f.confirmSsh.mockImplementation(
    () =>
      new Promise((resolve) => {
        finish = resolve;
      }),
  );
  const pending = f.controller.request(request);
  f.retire();
  f.life.abort();
  finish(true);
  await pending;
  expect(f.answer).not.toHaveBeenCalled();
  expect(f.opened).not.toHaveBeenCalled();
  f.life.abort();
});
