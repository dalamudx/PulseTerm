import { expect, it, vi } from "vitest";
import {
  decodeCustomBriefs,
  decodeCustomDraft,
  decodeCustomOffer,
  emptyCustomConfig,
  openCustomProfile,
} from "./workspace-custom-profiles";

const id = "01234567-89ab-7cde-8123-0123456789ab",
  nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const draft = () => ({
  name: "保存",
  executable: "/bin/sh",
  hidden: false,
  config: {
    ...emptyCustomConfig(),
    args: ["", " ; $(literal) 中文🌍 "],
    env: [["VALUE", "literal = value"]],
  },
});
it("keeps saved identities separate from Shell kinds and preserves literal configuration", () => {
  const value = decodeCustomOffer({
    ticket: nonce,
    profiles: [{ id, deleted: false, ...draft() }],
  });
  expect(value.profiles[0]?.config.args).toEqual(["", " ; $(literal) 中文🌍 "]);
  expect(decodeCustomBriefs([{ id, name: "保存", hidden: true }])).toEqual([
    { id, name: "保存", hidden: true },
  ]);
  expect(() =>
    decodeCustomOffer({
      ticket: nonce,
      profiles: [
        { id, deleted: false, ...draft() },
        { id, deleted: true, ...draft() },
      ],
    }),
  ).toThrow();
  for (const invalid of ["sh", nonce, id.toUpperCase(), "../x"])
    expect(() =>
      decodeCustomBriefs([{ id: invalid, name: "保存", hidden: false }]),
    ).toThrow();
  for (const change of [
    { executable: "sh" },
    { executable: "/bin/\nsh" },
    { name: " " },
    { name: "n".repeat(81) },
    { hidden: "true" },
  ])
    expect(() => decodeCustomDraft({ ...draft(), ...change })).toThrow();
});
it("sends only a saved UUID at startup and rejects malformed identities and receipts", async () => {
  const send = vi.fn(async () => "42");
  expect(await openCustomProfile(nonce, id, send)).toBe("42");
  expect(send).toHaveBeenCalledExactlyOnceWith(nonce, id);
  await expect(openCustomProfile(nonce, "sh", send)).rejects.toThrow();
  await expect(openCustomProfile("old-page", id, send)).rejects.toThrow();
  expect(send).toHaveBeenCalledTimes(1);
  for (const receipt of [
    nonce,
    "0",
    "01",
    "18446744073709551616",
    2,
    null,
    { id: "1" },
  ]) {
    await expect(
      openCustomProfile(nonce, id, async () => receipt),
    ).rejects.toThrow();
  }
});
