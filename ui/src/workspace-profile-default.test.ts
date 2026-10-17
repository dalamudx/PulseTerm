import { afterEach, expect, it, vi } from "vitest";
import {
  DefaultProfileEditor,
  decodeDefaultProfile,
} from "./workspace-profile-default";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const offer = {
  ticket,
  selected: "sh",
  available: true,
  offered: ["sh", "bash"],
};
function fixture(
  read: () => Promise<unknown> = async () => offer,
  save: () => Promise<unknown> = async () => null,
) {
  const life = new AbortController();
  const native = {
    read: vi.fn(read),
    save: vi.fn(save),
    cancel: vi.fn(async () => null),
  };
  return {
    life,
    native,
    editor: new DefaultProfileEditor(nonce, life.signal, native),
  };
}
afterEach(() => vi.useRealTimers());
it("strictly accepts unavailable saved identities without selecting a fallback", () => {
  expect(
    decodeDefaultProfile({ ...offer, selected: "fish", available: false })
      .selected,
  ).toBe("fish");
  expect(
    decodeDefaultProfile({ ...offer, selected: null, available: false })
      .selected,
  ).toBeNull();
  for (const value of [
    { ...offer, path: "/bin/sh" },
    { ...offer, ticket: "bad" },
    { ...offer, selected: "bad" },
    { ...offer, offered: ["sh", "sh"] },
    { ...offer, selected: "fish" },
    { ...offer, available: false },
  ])
    expect(() => decodeDefaultProfile(value)).toThrow();
});
it("sends only original nonce ticket and offered kind once; save does not launch", async () => {
  const { editor, native } = fixture();
  await editor.read();
  expect(await editor.save("fish")).toBe("denied");
  expect(await editor.save("bash")).toBe("saved");
  expect(native.save).toHaveBeenCalledExactlyOnceWith(nonce, ticket, "bash");
  expect(await editor.save("sh")).toBe("denied");
  expect(native.cancel).toHaveBeenCalledWith(nonce, ticket);
  editor.close();
});
it("clearing is an explicit null write", async () => {
  const { editor, native } = fixture();
  await editor.read();
  expect(await editor.save(null)).toBe("saved");
  expect(native.save).toHaveBeenCalledExactlyOnceWith(nonce, ticket, null);
  editor.close();
});
it("close before a late read cancels only that original ticket", async () => {
  let resolve!: (value: unknown) => void;
  const { editor, native, life } = fixture(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const job = editor.read();
  life.abort();
  expect(await job).toBeNull();
  resolve(offer);
  await Promise.resolve();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  expect(editor.offer).toBeNull();
  expect(await editor.save("sh")).toBe("denied");
});
it("read timeout rejects late publication and retires the eventual ticket", async () => {
  vi.useFakeTimers();
  let resolve!: (value: unknown) => void;
  const { editor, native } = fixture(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const job = editor.read();
  await vi.advanceTimersByTimeAsync(5000);
  expect(await job).toBeNull();
  resolve(offer);
  await Promise.resolve();
  expect(editor.offer).toBeNull();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  editor.close();
});
it("save timeout consumes authorization, revokes native, and never auto retries", async () => {
  vi.useFakeTimers();
  let resolve!: (value: unknown) => void;
  const { editor, native } = fixture(
    undefined,
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await editor.read();
  const job = editor.save("sh");
  await vi.advanceTimersByTimeAsync(5000);
  expect(await job).toBe("uncertain");
  resolve(null);
  await Promise.resolve();
  expect(await editor.save("sh")).toBe("denied");
  expect(native.save).toHaveBeenCalledTimes(1);
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  editor.close();
});
it("close during save revokes in-flight authorization without consuming late success", async () => {
  let resolve!: (value: unknown) => void;
  const { editor, native } = fixture(
    undefined,
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  await editor.read();
  const job = editor.save("sh");
  editor.close();
  expect(await job).toBe("cancelled");
  resolve(null);
  await Promise.resolve();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  expect(editor.offer).toBeNull();
});
it("local expiry cannot extend the original ticket", async () => {
  vi.useFakeTimers();
  const { editor, native } = fixture();
  await editor.read();
  await vi.advanceTimersByTimeAsync(55000);
  expect(await editor.save("sh")).toBe("denied");
  expect(native.save).not.toHaveBeenCalled();
  editor.close();
});
it("bad native receipt is uncertain and malformed read is revoked", async () => {
  const { editor, native } = fixture(async () => ({ ...offer, extra: true }));
  expect(await editor.read()).toBeNull();
  expect(native.cancel).toHaveBeenCalledExactlyOnceWith(nonce, ticket);
  editor.close();
  const second = fixture(undefined, async () => ({ ok: true }));
  await second.editor.read();
  expect(await second.editor.save("sh")).toBe("uncertain");
  second.editor.close();
});
it("replacement read retires prior ticket and stale result cannot replace the new one", async () => {
  let resolve!: (value: unknown) => void;
  const { editor, native } = fixture();
  native.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        resolve = r;
      }),
  );
  const old = editor.read();
  const current = await editor.read();
  expect(await old).toBeNull();
  const stale = "cccccccc-cccc-4ccc-8ccc-cccccccccccc";
  resolve({ ...offer, ticket: stale });
  await Promise.resolve();
  expect(editor.offer).toBe(current);
  expect(native.cancel).toHaveBeenCalledWith(nonce, stale);
  editor.close();
});

it("saved UUID defaults stay literal and unavailable identities never fall back", () => {
  const id = "019a0000-0000-7000-8000-000000000001";
  const base = { ticket, selected: null, offered: ["sh"], customSelected: id };
  const active = decodeDefaultProfile({
    ...base,
    available: true,
    customOffered: [{ id, name: "保存" }],
    customName: "保存",
  });
  expect(active.selected).toBe(id);
  expect(active.names?.[id]).toBe("保存");
  expect(
    decodeDefaultProfile({
      ...base,
      available: false,
      customOffered: [],
      customName: "保存",
    }).selected,
  ).toBe(id);
  expect(() =>
    decodeDefaultProfile({ ...base, available: true, customOffered: [] }),
  ).toThrow();
});
