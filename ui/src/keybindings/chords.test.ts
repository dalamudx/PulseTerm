import { expect, it } from "vitest";
import {
  type ChordKeyEvent,
  chordSequencesConflict,
  formatChordSequence,
  isModifiedAppChord,
  type KeyChord,
  matchesChord,
  parseChordSequence,
} from "./chords";

const first = (value: string) => parseChordSequence(value)[0] as KeyChord;
const event = (extra: Partial<ChordKeyEvent> = {}): ChordKeyEvent => ({
  key: "P",
  ctrlKey: true,
  metaKey: false,
  altKey: false,
  shiftKey: true,
  isComposing: false,
  defaultPrevented: false,
  ...extra,
});
it("canonicalizes modifier order and known names, preserving at most two steps", () => {
  expect(
    formatChordSequence(parseChordSequence(" Shift+CTRL+P  ctrl+s ")),
  ).toBe("ctrl+shift+p ctrl+s");
  expect(formatChordSequence(parseChordSequence("F12"))).toBe("f12");
});
it("rejects unknown payloads, syntax and non-ASCII rather than interpreting commands", () => {
  for (const value of [
    null,
    1,
    {},
    "",
    " ",
    "ctrl++p",
    "ctrl+p+shift",
    "ctrl+ctrl+p",
    "command+p",
    "super+p",
    "ctrl+é",
    "ctrl+😀",
    "ctrl+f13",
    "ctrl+;",
    "ctrl+a\nctrl+b",
    "ctrl+a ctrl+b ctrl+c",
    "x".repeat(129),
  ])
    expect(() => parseChordSequence(value)).toThrow();
});
it("parsed sequences and their entries are immutable", () => {
  const sequence = parseChordSequence("ctrl+shift+p");
  expect(Object.isFrozen(sequence)).toBe(true);
  expect(Object.isFrozen(sequence[0])).toBe(true);
});
it("exact and prefix collisions are symmetric in one context", () => {
  for (const [a, b] of [
    ["ctrl+k", "ctrl+k ctrl+s"],
    ["meta+shift+p", "shift+meta+p"],
    ["ctrl+k ctrl+s", "ctrl+k ctrl+s"],
  ]) {
    expect(
      chordSequencesConflict(
        parseChordSequence(a as string),
        parseChordSequence(b as string),
      ),
    ).toBe(true);
    expect(
      chordSequencesConflict(
        parseChordSequence(b as string),
        parseChordSequence(a as string),
      ),
    ).toBe(true);
  }
});
it("different complete sequences or modifier choices are not collisions", () => {
  expect(
    chordSequencesConflict(
      parseChordSequence("ctrl+k ctrl+s"),
      parseChordSequence("ctrl+k ctrl+p"),
    ),
  ).toBe(false);
  expect(
    chordSequencesConflict(
      parseChordSequence("ctrl+shift+p"),
      parseChordSequence("meta+shift+p"),
    ),
  ).toBe(false);
});
it("ordinary CtrlP/CtrlK and printable keys are not terminal app candidates", () => {
  for (const value of [
    "ctrl+p",
    "ctrl+k",
    "p",
    "shift+p",
    "alt+shift+p",
    "ctrl+meta+shift+p",
  ])
    expect(isModifiedAppChord(first(value))).toBe(false);
  expect(isModifiedAppChord(first("ctrl+shift+p"))).toBe(true);
  expect(isModifiedAppChord(first("meta+shift+p"))).toBe(true);
});
it("event matching retains modifiers and ignores IME/AltGraph/consumed input", () => {
  const chord = first("ctrl+shift+p");
  expect(matchesChord(chord, event())).toBe(true);
  for (const value of [
    { shiftKey: false },
    { ctrlKey: false },
    { metaKey: true },
    { altKey: true },
    { isComposing: true },
    { keyCode: 229 },
    { key: "Process" },
    { defaultPrevented: true },
    { getModifierState: (name: string) => name === "AltGraph" },
  ])
    expect(matchesChord(chord, event(value))).toBe(false);
});
it("space and navigation keys have explicit canonical names", () => {
  expect(
    matchesChord(
      first("space"),
      event({ key: " ", ctrlKey: false, shiftKey: false }),
    ),
  ).toBe(true);
  expect(
    matchesChord(
      first("ctrl+arrowleft"),
      event({ key: "ArrowLeft", shiftKey: false }),
    ),
  ).toBe(true);
});
it("formatting rejects malformed runtime flags and keys", () => {
  for (const chord of [
    { ...first("ctrl+p"), ctrl: "yes" },
    { ...first("ctrl+p"), key: "P" },
    { ...first("ctrl+p"), key: "bogus" },
  ])
    expect(() => formatChordSequence([chord as KeyChord])).toThrow();
  expect(() => formatChordSequence([])).toThrow();
});
it("all 16 modifier combinations and 36 printable keys roundtrip exactly", () => {
  for (let flags = 0; flags < 16; flags++)
    for (const key of "abcdefghijklmnopqrstuvwxyz0123456789") {
      const chord: KeyChord = {
        ctrl: !!(flags & 1),
        meta: !!(flags & 2),
        alt: !!(flags & 4),
        shift: !!(flags & 8),
        key,
      };
      expect(parseChordSequence(formatChordSequence([chord]))).toEqual([chord]);
    }
});
