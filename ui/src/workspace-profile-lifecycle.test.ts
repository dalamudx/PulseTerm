import { afterEach, expect, it, vi } from "vitest";
import {
  decodeDeletedProfiles,
  decodeLifecycleOffer,
  ProfileLifecycleEditor,
  type ProfileLifecycleNative,
} from "./workspace-profile-lifecycle";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const offer = { ticket, shell: "sh", action: "delete", isDefault: true };
function fixture() {
  const life = new AbortController();
  const native: ProfileLifecycleNative = {
    deleted: vi.fn(async () => []),
    prepare: vi.fn(async () => offer),
    commit: vi.fn(async () => null),
    cancel: vi.fn(async () => null),
  };
  return {
    life,
    native,
    editor: new ProfileLifecycleEditor(nonce, life.signal, native),
  };
}
afterEach(() => vi.useRealTimers());
it("strictly rejects malformed tombstones and capabilities", () => {
  expect(decodeDeletedProfiles(["sh", "bash"])).toEqual(["sh", "bash"]);
  for (const value of [
    null,
    ["sh", "sh"],
    ["rawpath"],
    [1],
    {},
    ["bash", "sh", "fish", "pwsh", "zsh", "bash"],
  ])
    expect(() => decodeDeletedProfiles(value)).toThrow();
  expect(decodeLifecycleOffer(offer)).toEqual(offer);
  for (const value of [
    null,
    {},
    { ...offer, extra: true },
    { ...offer, shell: "/bin/sh" },
    { ...offer, action: "launch" },
    { ...offer, ticket: 1 },
    { ...offer, isDefault: "yes" },
  ])
    expect(() => decodeLifecycleOffer(value)).toThrow();
});
it("fixed target preparation is read-only and commit uses only one ticket", async () => {
  const f = fixture();
  await f.editor.prepare("sh", "delete");
  expect(f.native.commit).not.toHaveBeenCalled();
  const saving = f.editor.commit();
  expect(await f.editor.commit()).toBe("denied");
  expect(await saving).toBe("saved");
  expect(f.native.commit).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  expect(f.native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  f.editor.close();
});
it.each([
  { ...offer, shell: "bash" },
  { ...offer, action: "restore" },
  { ...offer, extra: true },
])(
  "rejects substituted or malformed preparation and cancels original ticket %j",
  async (value) => {
    const f = fixture();
    vi.mocked(f.native.prepare).mockResolvedValue(value);
    expect(await f.editor.prepare("sh", "delete")).toBeNull();
    expect(f.native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
    expect(await f.editor.commit()).toBe("denied");
    f.editor.close();
  },
);
it.each(["close", "page", "timeout"])(
  "late prepare after %s cancels ticket and never reinstalls authority",
  async (mode) => {
    vi.useFakeTimers();
    const f = fixture();
    let resolve!: (value: unknown) => void;
    vi.mocked(f.native.prepare).mockImplementation(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    const read = f.editor.prepare("sh", "delete");
    if (mode === "close") f.editor.close();
    else if (mode === "page") f.life.abort();
    else await vi.advanceTimersByTimeAsync(5000);
    expect(await read).toBeNull();
    resolve(offer);
    await Promise.resolve();
    expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
    expect(f.editor.offer).toBeNull();
    f.editor.close();
  },
);
it("replacement preparation cancels both superseded receipts without canceling new ticket", async () => {
  const f = fixture();
  let resolve!: (v: unknown) => void;
  vi.mocked(f.native.prepare).mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const first = f.editor.prepare("sh", "delete");
  const secondTicket = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
  vi.mocked(f.native.prepare).mockResolvedValue({
    ...offer,
    ticket: secondTicket,
    shell: "bash",
  });
  await f.editor.prepare("bash", "delete");
  expect(await first).toBeNull();
  resolve(offer);
  await Promise.resolve();
  expect(f.editor.offer?.ticket).toBe(secondTicket);
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.native.cancel).not.toHaveBeenCalledWith(nonce, secondTicket);
  f.editor.close();
});
it("local confirmation expiry cannot be used", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.editor.prepare("sh", "delete");
  await vi.advanceTimersByTimeAsync(55000);
  expect(await f.editor.commit()).toBe("denied");
  expect(f.native.commit).not.toHaveBeenCalled();
  f.editor.close();
});
it.each(["timeout", "close", "page"])(
  "in-flight commit %s cancels and ignores late success without retry",
  async (mode) => {
    vi.useFakeTimers();
    const f = fixture();
    let resolve!: (v: unknown) => void;
    vi.mocked(f.native.commit).mockImplementation(
      () =>
        new Promise((r) => {
          resolve = r;
        }),
    );
    await f.editor.prepare("sh", "delete");
    const saving = f.editor.commit();
    if (mode === "timeout") await vi.advanceTimersByTimeAsync(5000);
    else if (mode === "close") f.editor.close();
    else f.life.abort();
    expect(await saving).toBe(mode === "timeout" ? "uncertain" : "cancelled");
    expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
    resolve(null);
    await Promise.resolve();
    expect(await f.editor.commit()).toBe("denied");
    expect(f.native.commit).toHaveBeenCalledOnce();
    f.editor.close();
  },
);
it.each(["reject", "invalid"])(
  "failure never authorizes automatic retry",
  async (response) => {
    const f = fixture();
    vi.mocked(f.native.commit).mockImplementation(async () => {
      if (response === "reject") throw Error("conflict");
      return { retry: true };
    });
    await f.editor.prepare("sh", "delete");
    expect(await f.editor.commit()).toBe("uncertain");
    expect(await f.editor.commit()).toBe("denied");
    f.editor.close();
  },
);
