import { expect, it, vi } from "vitest";
import { loadTabLabels, saveTabLabels, tabLabel } from "./workspace-tab-labels";

it("titles are bounded plain text, reject control/bidi/invisible/surrogate or arbitrary CSS", () => {
  expect(tabLabel("  build <script> 中文🙂  ", "cyan")).toEqual({
    title: "build <script> 中文🙂",
    color: "cyan",
  });
  for (const title of [
    "x".repeat(81),
    "中".repeat(86),
    "a\ncmd",
    "a\x1b",
    "a\u202e",
    "\ufeffabc",
    "\nabc",
    "a\ud800",
  ])
    expect(() => tabLabel(title, "none")).toThrow();
  for (const color of ["#ff0000", "url(javascript:x)", "__proto__"])
    expect(() => tabLabel("title", color)).toThrow();
});
it("only current catalog identities survive reload, labels never create sessions", () => {
  const raw = JSON.stringify({
    version: 1,
    entries: [
      { session: "1", title: "kept", color: "red" },
      { session: "2", title: "old", color: "blue" },
    ],
  });
  expect([...loadTabLabels(["1"], { getItem: () => raw })]).toEqual([
    ["1", { title: "kept", color: "red" }],
  ]);
  expect(loadTabLabels([], { getItem: () => raw }).size).toBe(0);
});
it("corrupt/future/oversize/duplicate maps fail closed without partial preference publication", () => {
  for (const value of [
    "{",
    "x".repeat(32769),
    JSON.stringify({ version: 2, entries: [] }),
    JSON.stringify({
      version: 1,
      entries: [
        { session: "1", title: "ok", color: "none" },
        { session: "1", title: "duplicate", color: "red" },
      ],
    }),
    JSON.stringify({
      version: 1,
      entries: [
        { session: "1", title: "ok", color: "none" },
        { session: "2", title: "bad\n", color: "red" },
      ],
    }),
  ])
    expect(loadTabLabels(["1"], { getItem: () => value }).size).toBe(0);
});
it("u64 ids remain exact, storage failure only limits display persistence", () => {
  const labels = new Map([
    ["18446744073709551615", tabLabel("compile", "green")],
  ]);
  let raw = "";
  expect(
    saveTabLabels([...labels.keys()], labels, {
      setItem: (_key, value) => {
        raw = value;
      },
    }),
  ).toBe(true);
  expect([
    ...loadTabLabels([...labels.keys()], { getItem: () => raw }),
  ]).toEqual([...labels]);
  expect(
    saveTabLabels([...labels.keys()], labels, {
      setItem() {
        throw Error("unavailable");
      },
    }),
  ).toBe(false);
  expect(
    loadTabLabels(["1"], {
      getItem() {
        throw Error("unavailable");
      },
    }).size,
  ).toBe(0);
});
it("stale or forged target rejected before display preference storage write", () => {
  const setItem = vi.fn();
  expect(() =>
    saveTabLabels(["1"], new Map([["2", tabLabel("old", "red")]]), { setItem }),
  ).toThrow("label_stale");
  expect(() =>
    saveTabLabels(["1"], new Map([["01", tabLabel("bad", "red")]]), {
      setItem,
    }),
  ).toThrow();
  expect(setItem).not.toHaveBeenCalled();
});
