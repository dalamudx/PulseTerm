import { expect, it } from "vitest";
import { profileDisplay } from "../../workspace-profile-editor";
import { displayPreferences } from "./display-preferences";

const base = { fontSize: 14, fontFamily: "monospace", cursorBlink: false };
it("safe immutable fixed fonts and explicit false", () => {
  const r = displayPreferences(base);
  expect(r).toEqual(base);
  expect(Object.isFrozen(r)).toBe(true);
  expect(
    displayPreferences({ ...base, fontFamily: "等宽字体" }).fontFamily,
  ).toBe("等宽字体");
});
it("unsafe CSS/font URLs/missing/future/oversize/numeric extremes rejected", () => {
  for (const v of [
    { ...base, fontSize: 5 },
    { ...base, fontSize: 73 },
    { ...base, fontSize: 14.5 },
    { ...base, fontSize: NaN },
    { ...base, fontFamily: "mono;url(x)" },
    { ...base, fontFamily: "x".repeat(129) },
    { ...base, cursorBlink: 0 },
    { ...base, extra: true },
    { fontSize: 14 },
  ])
    expect(() => displayPreferences(v)).toThrow("terminal.display_invalid");
});

it("cell metrics stay drawing-only, bounded and immutable through sparse Profile merging", () => {
  const draw = displayPreferences({
    ...base,
    lineHeightPercent: 150,
    letterSpacing: 0,
  });
  expect(draw.lineHeightPercent).toBe(150);
  expect(draw.letterSpacing).toBe(0);
  expect(Object.isFrozen(draw)).toBe(true);
  expect(
    profileDisplay(draw, { fontSize: 18, fontFamily: null, cursorBlink: null }),
  ).toMatchObject({ fontSize: 18, lineHeightPercent: 150, letterSpacing: 0 });
  for (const value of [
    { ...base, lineHeightPercent: 99 },
    { ...base, lineHeightPercent: 301 },
    { ...base, lineHeightPercent: 150.5 },
    { ...base, lineHeightPercent: NaN },
    { ...base, letterSpacing: -1 },
    { ...base, letterSpacing: 9 },
    { ...base, letterSpacing: 0.5 },
    { ...base, letterSpacing: undefined },
  ])
    expect(() => displayPreferences(value)).toThrow("terminal.display_invalid");
});

it("Option Meta accepts explicit booleans and rejects malformed display preferences", () => {
  expect(displayPreferences(base).macOptionIsMeta).toBeUndefined();
  for (const value of [false, true]) {
    expect(
      displayPreferences({ ...base, macOptionIsMeta: value }).macOptionIsMeta,
    ).toBe(value);
  }
  for (const value of ["false", 0, null, undefined]) {
    expect(() =>
      displayPreferences({ ...base, macOptionIsMeta: value }),
    ).toThrow("terminal.display_invalid");
  }
});

it("frozen SSH behavior overrides later global preferences during recovery", () => {
  const recovered = profileDisplay(
    { ...base, macOptionIsMeta: true, backspaceSend: "del", deleteSend: "csi" },
    {
      fontSize: null,
      fontFamily: null,
      cursorBlink: null,
      macOptionIsMeta: false,
      backspaceSend: "bs",
      deleteSend: "del",
    },
  );
  expect(recovered).toMatchObject({
    macOptionIsMeta: false,
    backspaceSend: "bs",
    deleteSend: "del",
  });
});

it("confirmClose accepts explicit booleans and rejects non-boolean values", () => {
  expect(displayPreferences({ ...base, confirmClose: true }).confirmClose).toBe(
    true,
  );
  expect(
    displayPreferences({ ...base, confirmClose: false }).confirmClose,
  ).toBe(false);
  expect(displayPreferences(base).confirmClose).toBeUndefined();
  for (const value of ["false", 0, null, 1]) {
    expect(() => displayPreferences({ ...base, confirmClose: value })).toThrow(
      "terminal.display_invalid",
    );
  }
});
