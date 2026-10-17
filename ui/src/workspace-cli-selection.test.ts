import { afterEach, expect, it, vi } from "vitest";
import {
  CliSelectionController,
  decodeCliSelection,
} from "./workspace-cli-selection";

const nonce = "11111111-1111-4111-8111-111111111111";
const ticket = "22222222-2222-4222-8222-222222222222";
const packet = { version: 1, nonce, ticket, session: "7" };
afterEach(() => vi.useRealTimers());
function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}
function fixture() {
  const life = new AbortController();
  let current: string | undefined = nonce,
    available = true;
  const admit = vi.fn(async () => {}),
    select = vi.fn(async () => true),
    complete = vi.fn(async () => {});
  const controller = new CliSelectionController({
    signal: life.signal,
    current: () => current,
    available: () => available,
    admit,
    select,
    complete,
  });
  return {
    controller,
    life,
    admit,
    select,
    complete,
    setCurrent(value: string | undefined) {
      current = value;
    },
    setAvailable(value: boolean) {
      available = value;
    },
  };
}
it("requires exact native admission, selection completion and one reply", async () => {
  const f = fixture();
  expect(await f.controller.request(packet)).toBe(true);
  expect(f.admit).toHaveBeenCalledOnce();
  expect(f.select).toHaveBeenCalledOnce();
  expect(f.complete).toHaveBeenCalledExactlyOnceWith(
    { nonce, ticket, session: "7" },
    true,
  );
  expect(f.admit.mock.invocationCallOrder[0]).toBeLessThan(
    f.select.mock.invocationCallOrder[0] ?? 0,
  );
  expect(f.select.mock.invocationCallOrder[0]).toBeLessThan(
    f.complete.mock.invocationCallOrder[0] ?? 0,
  );
});
it("accepts U64 without JS numeric truncation and freezes decoded metadata", () => {
  expect(
    decodeCliSelection({ ...packet, session: "18446744073709551615" }).session,
  ).toBe("18446744073709551615");
  expect(Object.isFrozen(decodeCliSelection(packet))).toBe(true);
});
it("rejects malformed versions, extra capabilities, foreign IDs and controls", async () => {
  const f = fixture();
  for (const value of [
    null,
    [],
    {},
    { ...packet, version: 2 },
    { ...packet, nonce: "bad" },
    { ...packet, ticket: "bad" },
    { ...packet, ticket: "abcdefab-abcd-4abc-8abc-abcdefabcdef".toUpperCase() },
    ...[0, "0", "01", "+7", " 7", "7\ncmd", "18446744073709551616"].map(
      (session) => ({ ...packet, session }),
    ),
    { ...packet, path: "/bin/sh" },
    { ...packet, profile: "sh" },
  ]) {
    expect(await f.controller.request(value)).toBe(false);
  }
  expect(f.admit).not.toHaveBeenCalled();
  expect(f.select).not.toHaveBeenCalled();
  expect(f.complete).not.toHaveBeenCalled();
});
it("original nonce and page lifetime gate all requests", async () => {
  for (const current of [undefined, ticket]) {
    const f = fixture();
    f.setCurrent(current);
    expect(await f.controller.request(packet)).toBe(false);
    expect(f.admit).not.toHaveBeenCalled();
  }
  const f = fixture();
  f.life.abort();
  expect(await f.controller.request(packet)).toBe(false);
  expect(f.complete).not.toHaveBeenCalled();
});
it("busy/modal rejection only denies and never selects", async () => {
  const f = fixture();
  f.setAvailable(false);
  expect(await f.controller.request(packet)).toBe(false);
  expect(f.admit).not.toHaveBeenCalled();
  expect(f.select).not.toHaveBeenCalled();
  expect(f.complete).toHaveBeenCalledExactlyOnceWith(
    { nonce, ticket, session: "7" },
    false,
  );
});
it("forged or replayed native tickets cannot cause UI effect", async () => {
  const f = fixture();
  f.admit.mockRejectedValue(Error("denied"));
  expect(await f.controller.request(packet)).toBe(false);
  expect(f.select).not.toHaveBeenCalled();
  expect(f.complete).not.toHaveBeenCalled();
});
it("one pending operation refuses duplicates without cancelling original", async () => {
  const f = fixture(),
    wait = deferred<void>();
  f.admit.mockReturnValue(wait.promise);
  const first = f.controller.request(packet);
  expect(await f.controller.request(packet)).toBe(false);
  expect(f.admit).toHaveBeenCalledOnce();
  wait.resolve();
  expect(await first).toBe(true);
});
it("packet mutation cannot replace the admitted target", async () => {
  const f = fixture(),
    wait = deferred<void>(),
    mutable = { ...packet };
  f.admit.mockReturnValue(wait.promise);
  const work = f.controller.request(mutable);
  mutable.session = "8";
  wait.resolve();
  expect(await work).toBe(true);
  expect(f.complete).toHaveBeenCalledExactlyOnceWith(
    { nonce, ticket, session: "7" },
    true,
  );
});
it("rechecks modal/busy state after native admission", async () => {
  const f = fixture(),
    wait = deferred<void>();
  f.admit.mockReturnValue(wait.promise);
  const work = f.controller.request(packet);
  f.setAvailable(false);
  wait.resolve();
  expect(await work).toBe(false);
  expect(f.select).not.toHaveBeenCalled();
  expect(f.complete).toHaveBeenCalledExactlyOnceWith(
    { nonce, ticket, session: "7" },
    false,
  );
});
it("refresh or abort during admission cannot borrow a late result", async () => {
  for (const abort of [false, true]) {
    const f = fixture(),
      wait = deferred<void>();
    f.admit.mockReturnValue(wait.promise);
    const work = f.controller.request(packet);
    if (abort) f.life.abort();
    else f.setCurrent(ticket);
    wait.resolve();
    expect(await work).toBe(false);
    expect(f.select).not.toHaveBeenCalled();
    expect(f.complete).not.toHaveBeenCalled();
  }
});
it("only exact ticket cancellation revokes pending operation", async () => {
  const f = fixture(),
    wait = deferred<void>();
  f.admit.mockReturnValue(wait.promise);
  const work = f.controller.request(packet);
  f.controller.cancel({ ticket: nonce });
  f.controller.cancel({ ticket, extra: true });
  wait.resolve();
  expect(await work).toBe(true);
  const g = fixture(),
    late = deferred<void>();
  g.admit.mockReturnValue(late.promise);
  const old = g.controller.request(packet);
  g.controller.cancel({ ticket });
  late.resolve();
  expect(await old).toBe(false);
  expect(g.select).not.toHaveBeenCalled();
});
it("five-second timeout cannot turn late admission into selection", async () => {
  vi.useFakeTimers();
  const f = fixture(),
    wait = deferred<void>();
  f.admit.mockReturnValue(wait.promise);
  const work = f.controller.request(packet);
  await vi.advanceTimersByTimeAsync(5_000);
  wait.resolve();
  expect(await work).toBe(false);
  expect(f.select).not.toHaveBeenCalled();
  expect(f.complete).not.toHaveBeenCalled();
});
it("cancelled in-flight selection cannot publish success", async () => {
  const f = fixture(),
    wait = deferred<boolean>();
  f.select.mockReturnValue(wait.promise);
  const work = f.controller.request(packet);
  await Promise.resolve();
  f.controller.cancel({ ticket });
  wait.resolve(true);
  expect(await work).toBe(false);
  expect(f.complete).not.toHaveBeenCalled();
});
it("failed selection reports false once, without action retry", async () => {
  for (const throws of [false, true]) {
    const f = fixture();
    if (throws) f.select.mockRejectedValue(Error("failed"));
    else f.select.mockResolvedValue(false);
    expect(await f.controller.request(packet)).toBe(false);
    expect(f.select).toHaveBeenCalledOnce();
    expect(f.complete).toHaveBeenCalledExactlyOnceWith(
      { nonce, ticket, session: "7" },
      false,
    );
  }
});
it("uncertain completion never repeats selection or sends undo", async () => {
  const f = fixture();
  f.complete.mockRejectedValue(Error("uncertain"));
  expect(await f.controller.request(packet)).toBe(false);
  expect(f.select).toHaveBeenCalledOnce();
  expect(f.complete).toHaveBeenCalledOnce();
});
it("retired page cannot publish local success from late completion", async () => {
  const f = fixture(),
    wait = deferred<void>();
  f.complete.mockReturnValue(wait.promise);
  const work = f.controller.request(packet);
  await Promise.resolve();
  await Promise.resolve();
  f.life.abort();
  wait.resolve();
  expect(await work).toBe(false);
  expect(f.complete).toHaveBeenCalledOnce();
});
