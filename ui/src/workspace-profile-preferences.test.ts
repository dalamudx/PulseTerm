import { afterEach, expect, it, vi } from "vitest";
import { changeProfileVisibility } from "./workspace-profile-preferences";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  offered = [{ kind: "sh", hidden: false }] as const;
afterEach(() => vi.useRealTimers());
it("only explicit current offered identity and boolean go to native, never path or snapshot", async () => {
  const send = vi.fn(async () => null);
  expect(
    await changeProfileVisibility(
      nonce,
      "sh",
      true,
      offered,
      new AbortController().signal,
      send,
    ),
  ).toBe("saved");
  expect(send).toHaveBeenCalledExactlyOnceWith(nonce, "sh", true);
});
it("old page stops consuming late receipt without automatically reversing or retrying a possibly committed edit", async () => {
  const life = new AbortController();
  let resolve!: (value: unknown) => void;
  const send = vi.fn(
    () =>
      new Promise<unknown>((r) => {
        resolve = r;
      }),
  );
  const job = changeProfileVisibility(
    nonce,
    "sh",
    true,
    offered,
    life.signal,
    send,
  );
  life.abort();
  expect(await job).toBe("cancelled");
  resolve(null);
  await Promise.resolve();
  expect(send).toHaveBeenCalledTimes(1);
});
it("timeouts/error/unrecognized receipt are uncertain, not auto-replayed", async () => {
  vi.useFakeTimers();
  const send = vi.fn(() => new Promise<unknown>(() => {}));
  const job = changeProfileVisibility(
    nonce,
    "sh",
    true,
    offered,
    new AbortController().signal,
    send,
  );
  await vi.advanceTimersByTimeAsync(5000);
  expect(await job).toBe("uncertain");
  expect(send).toHaveBeenCalledTimes(1);
  for (const native of [
    async () => ({ hidden: true }),
    async () => {
      throw Error("error");
    },
  ])
    expect(
      await changeProfileVisibility(
        nonce,
        "sh",
        true,
        offered,
        new AbortController().signal,
        native,
      ),
    ).toBe("uncertain");
});
it("bad nonce or non-discovered kind rejects before native and pre-aborted never sends", async () => {
  const send = vi.fn(async () => null),
    life = new AbortController();
  await expect(
    changeProfileVisibility("bad", "sh", true, offered, life.signal, send),
  ).rejects.toThrow();
  await expect(
    changeProfileVisibility(nonce, "fish", true, offered, life.signal, send),
  ).rejects.toThrow();
  expect(send).not.toHaveBeenCalled();
  life.abort();
  expect(
    await changeProfileVisibility(
      nonce,
      "sh",
      true,
      offered,
      life.signal,
      send,
    ),
  ).toBe("cancelled");
});
