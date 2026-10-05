import { expect, it } from "vitest";
import type { ShellHints } from "./gen/commands";
import { assertShellHintReceipt } from "./shell-hint-receipt";

const base: ShellHints = {
  sampledRevision: "18446744073709551615",
  sampledSourceBytes: "7",
  sampledDisplayBytes: "9",
  dropped: "0",
  markers: [],
  loginFixture: false,
  initialPage: true,
};
it("keeps uint64 and dual sampling coordinates without treating them as hint positions", () => {
  expect(() => assertShellHintReceipt(base, true)).not.toThrow();
});
it("requires all three fixed login hints once and empty metadata in a fresh page", () => {
  const markers: ShellHints["markers"] = [
    { kind: "promptStart" },
    { kind: "promptEnd" },
    { kind: "commandStart" },
  ];
  expect(() =>
    assertShellHintReceipt({ ...base, loginFixture: true, markers }, true),
  ).not.toThrow();
  expect(() =>
    assertShellHintReceipt({ ...base, loginFixture: true, markers: [] }, true),
  ).toThrow();
  expect(() =>
    assertShellHintReceipt(
      { ...base, loginFixture: true, initialPage: false },
      false,
    ),
  ).not.toThrow();
  expect(() =>
    assertShellHintReceipt(
      { ...base, loginFixture: true, initialPage: false, markers },
      false,
    ),
  ).toThrow();
});
it("rejects malformed, overflowing and coerced coordinates or flags", () => {
  for (const s of ["01", "-1", "1.0", "18446744073709551616", " 1"])
    expect(() =>
      assertShellHintReceipt({ ...base, sampledRevision: s }, true),
    ).toThrow();
  for (const patch of [
    { dropped: "1" },
    { initialPage: false },
    { loginFixture: "true" },
    { sampledDisplayBytes: 9 },
  ])
    expect(() =>
      assertShellHintReceipt({ ...base, ...patch } as ShellHints, true),
    ).toThrow();
});
it("does not accept an advisory command exit as the fixed fixture proof", () => {
  expect(() =>
    assertShellHintReceipt(
      {
        ...base,
        loginFixture: true,
        markers: [{ kind: "commandEnd", exitStatus: 0 }],
      },
      true,
    ),
  ).toThrow();
});
