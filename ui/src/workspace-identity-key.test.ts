import { afterEach, beforeEach, expect, it, vi } from "vitest";

const native = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke: native }));

import { keyImportAvailable, saveIdentityKey } from "./workspace-identity-key";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  identity = "bbbbbbbb-bbbb-7bbb-8bbb-bbbbbbbbbbbb",
  ticket = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
beforeEach(() => {
  native.mockReset();
});
afterEach(() => vi.useRealTimers());
const fresh = () => new AbortController();
it.each([false, true])(
  "sends only exact key/ticket data, wipes both buffers and wire copies (passphrase=%s)",
  async (withPass) => {
    let keyCopy: number[] = [],
      passCopy: number[] | null = null;
    native.mockImplementation(
      (command: string, args: Record<string, unknown>) => {
        if (command.endsWith("prepare"))
          return Promise.resolve({ version: 1, ticket });
        if (command.endsWith("submit")) {
          keyCopy = [...(args.privateKey as number[])];
          passCopy =
            args.passphrase === null
              ? null
              : [...(args.passphrase as number[])];
          return Promise.resolve({ version: 1 });
        }
        return Promise.resolve();
      },
    );
    const key = new Uint8Array([1, 2, 3]),
      pass = withPass ? new Uint8Array([4, 5]) : undefined;
    await saveIdentityKey(nonce, identity, key, pass, fresh().signal);
    expect(keyCopy).toEqual([1, 2, 3]);
    expect(passCopy).toEqual(withPass ? [4, 5] : null);
    expect([...key]).toEqual([0, 0, 0]);
    if (pass) expect([...pass]).toEqual([0, 0]);
    expect(native.mock.calls[0]?.[1]).toEqual({ nonce, identity });
    const submit = native.mock.calls.find((c) => c[0].endsWith("submit"));
    expect(Object.keys(submit?.[1]).sort()).toEqual([
      "nonce",
      "passphrase",
      "privateKey",
      "ticket",
    ]);
    expect(submit?.[1].privateKey).toEqual([0, 0, 0]);
    expect(submit?.[1].passphrase).toEqual(withPass ? [0, 0] : null);
    expect(native).toHaveBeenCalledTimes(2);
  },
);
it.each([
  [0, undefined],
  [65537, undefined],
  [1, 0],
  [1, 4097],
] as const)(
  "rejects bounds (%s,%s) before admission and clears inputs",
  async (size, passSize) => {
    const key = new Uint8Array(size).fill(7),
      pass =
        passSize === undefined ? undefined : new Uint8Array(passSize).fill(8);
    await expect(
      saveIdentityKey(nonce, identity, key, pass, fresh().signal),
    ).rejects.toThrow("invalid");
    expect(native).not.toHaveBeenCalled();
    expect(key.every((b) => b === 0)).toBe(true);
    expect(pass?.every((b) => b === 0) ?? true).toBe(true);
  },
);
it("cancels late prepare using original nonce and never submits after close", async () => {
  let finish: (value: unknown) => void = () => {};
  native.mockImplementation((command: string) =>
    command.endsWith("prepare")
      ? new Promise((resolve) => {
          finish = resolve;
        })
      : Promise.resolve(),
  );
  const controller = fresh(),
    key = new Uint8Array([1]),
    pass = new Uint8Array([2]);
  const pending = saveIdentityKey(
    nonce,
    identity,
    key,
    pass,
    controller.signal,
  );
  controller.abort();
  await expect(pending).rejects.toThrow("cancelled");
  finish({ version: 1, ticket });
  await Promise.resolve();
  await Promise.resolve();
  expect(native.mock.calls.some((c) => c[0].endsWith("submit"))).toBe(false);
  expect(native.mock.calls.filter((c) => c[0].endsWith("cancel"))).toEqual([
    ["workspace_identity_key_cancel", { nonce, ticket }],
  ]);
  expect([...key, ...pass]).toEqual([0, 0]);
});
it("uncertain submit expires at five seconds and never retries", async () => {
  vi.useFakeTimers();
  native.mockImplementation((command: string) =>
    command.endsWith("prepare")
      ? Promise.resolve({ version: 1, ticket })
      : command.endsWith("submit")
        ? new Promise(() => {})
        : Promise.resolve(),
  );
  const key = new Uint8Array([1]),
    pass = new Uint8Array([2]);
  const pending = saveIdentityKey(nonce, identity, key, pass, fresh().signal);
  const rejection = expect(pending).rejects.toThrow("uncertain");
  await vi.advanceTimersByTimeAsync(5000);
  await rejection;
  expect(native.mock.calls.filter((c) => c[0].endsWith("submit"))).toHaveLength(
    1,
  );
  expect(native.mock.calls.filter((c) => c[0].endsWith("cancel"))).toHaveLength(
    1,
  );
  expect([...key, ...pass]).toEqual([0, 0]);
});
it.each([{ version: 2 }, { version: 1, privateKey: "must-not-echo" }])(
  "rejects secret/invalid submit reply without echo or retry",
  async (reply) => {
    native
      .mockResolvedValueOnce({ version: 1, ticket })
      .mockResolvedValueOnce(reply)
      .mockResolvedValue(undefined);
    await expect(
      saveIdentityKey(
        nonce,
        identity,
        new Uint8Array([1]),
        undefined,
        fresh().signal,
      ),
    ).rejects.toThrow("protocol");
    expect(
      native.mock.calls.filter((c) => c[0].endsWith("submit")),
    ).toHaveLength(1);
  },
);
it("sanitizes native errors and clears both buffers", async () => {
  native
    .mockResolvedValueOnce({ version: 1, ticket })
    .mockRejectedValueOnce(Error("must-not-echo-private-material"))
    .mockResolvedValue(undefined);
  const key = new Uint8Array([1]),
    pass = new Uint8Array([2]);
  await expect(
    saveIdentityKey(nonce, identity, key, pass, fresh().signal),
  ).rejects.toThrow("identity.key_failed");
  expect([...key, ...pass]).toEqual([0, 0]);
});
it.each([false, true])(
  "availability=%s is metadata only and creates no ticket",
  async (enabled) => {
    native.mockResolvedValue({ version: 1, enabled });
    await expect(keyImportAvailable(nonce, fresh().signal)).resolves.toBe(
      enabled,
    );
    expect(native.mock.calls).toEqual([
      ["workspace_identity_key_available", { nonce }],
    ]);
  },
);
it("aborting availability rejects without waiting for native reply", async () => {
  native.mockImplementation(() => new Promise(() => {}));
  const controller = fresh();
  const pending = keyImportAvailable(nonce, controller.signal);
  controller.abort();
  await expect(pending).rejects.toThrow("cancelled");
});
it("availability times out without enabling or making new requests", async () => {
  vi.useFakeTimers();
  native.mockImplementation(() => new Promise(() => {}));
  const pending = keyImportAvailable(nonce, fresh().signal);
  const rejection = expect(pending).rejects.toThrow("uncertain");
  await vi.advanceTimersByTimeAsync(5000);
  await rejection;
  expect(native).toHaveBeenCalledTimes(1);
});
it.each([
  { version: 1, enabled: "true" },
  { version: 1, enabled: true, key: "forbidden" },
])("malformed availability cannot enable imports", async (value) => {
  native.mockResolvedValue(value);
  await expect(keyImportAvailable(nonce, fresh().signal)).rejects.toThrow(
    "protocol",
  );
});
