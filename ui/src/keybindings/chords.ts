import { isCompositionKey } from "./composition";

const modifiers = ["ctrl", "meta", "alt", "shift"] as const;
export interface KeyChord {
  readonly ctrl: boolean;
  readonly meta: boolean;
  readonly alt: boolean;
  readonly shift: boolean;
  readonly key: string;
}
export type ChordSequence = readonly KeyChord[];
const namedKeys = new Set([
  "enter",
  "escape",
  "tab",
  "space",
  "backspace",
  "delete",
  "insert",
  "home",
  "end",
  "pageup",
  "pagedown",
  "arrowleft",
  "arrowright",
  "arrowup",
  "arrowdown",
]);
function validKey(key: string): boolean {
  return (
    /^[a-z0-9]$/.test(key) ||
    /^f(?:[1-9]|1[0-2])$/.test(key) ||
    namedKeys.has(key)
  );
}
function validChord(chord: KeyChord): boolean {
  return (
    !!chord &&
    typeof chord.key === "string" &&
    validKey(chord.key) &&
    modifiers.every((modifier) => typeof chord[modifier] === "boolean")
  );
}
/** Bounded syntax only. A parsed chord grants neither action nor terminal interception. */
export function parseChordSequence(value: unknown): ChordSequence {
  if (
    typeof value !== "string" ||
    !value.length ||
    value.length > 128 ||
    !/^[\x20-\x7e]+$/.test(value)
  )
    throw new Error("keymap.invalid_sequence");
  const steps = value.trim().toLowerCase().split(/ +/);
  if (!steps.length || steps.length > 2 || steps.some((step) => !step))
    throw new Error("keymap.invalid_sequence");
  return Object.freeze(
    steps.map((step) => {
      const parts = step.split("+"),
        key = parts.pop();
      if (
        !key ||
        !validKey(key) ||
        parts.length > 4 ||
        new Set(parts).size !== parts.length ||
        parts.some((part) => !modifiers.some((modifier) => modifier === part))
      )
        throw new Error("keymap.invalid_chord");
      return Object.freeze({
        ctrl: parts.includes("ctrl"),
        meta: parts.includes("meta"),
        alt: parts.includes("alt"),
        shift: parts.includes("shift"),
        key,
      });
    }),
  );
}
export function formatChordSequence(sequence: ChordSequence): string {
  if (
    !sequence.length ||
    sequence.length > 2 ||
    sequence.some((chord) => !validChord(chord))
  )
    throw new Error("keymap.invalid_sequence");
  const value = sequence
    .map((chord) =>
      [...modifiers.filter((modifier) => chord[modifier]), chord.key].join("+"),
    )
    .join(" ");
  // Validate runtime objects as well, including a noncanonical or malformed key.
  const parsed = parseChordSequence(value);
  if (
    parsed.some(
      (chord, index) =>
        modifiers.some(
          (modifier) => chord[modifier] !== sequence[index]?.[modifier],
        ) || chord.key !== sequence[index]?.key,
    )
  )
    throw new Error("keymap.invalid_chord");
  return value;
}
export function chordSequencesConflict(
  left: ChordSequence,
  right: ChordSequence,
): boolean {
  const a = parseChordSequence(formatChordSequence(left)),
    b = parseChordSequence(formatChordSequence(right));
  // An exact binding and its longer prefix cannot both win in the same context.
  return a.slice(0, Math.min(a.length, b.length)).every((chord, index) => {
    const other = b[index];
    return (
      !!other && formatChordSequence([chord]) === formatChordSequence([other])
    );
  });
}
/** Candidate policy, not proof of Shell compatibility; registry reservations still apply. */
export function isModifiedAppChord(chord: KeyChord): boolean {
  return (
    validChord(chord) &&
    !chord.alt &&
    chord.shift &&
    chord.ctrl !== chord.meta &&
    /^[a-z0-9]$/.test(chord.key)
  );
}
export interface ChordKeyEvent {
  readonly key: string;
  readonly ctrlKey: boolean;
  readonly metaKey: boolean;
  readonly altKey: boolean;
  readonly shiftKey: boolean;
  readonly isComposing: boolean;
  readonly keyCode?: number;
  readonly defaultPrevented: boolean;
  getModifierState?(modifier: string): boolean;
}
export function matchesChord(chord: KeyChord, event: ChordKeyEvent): boolean {
  if (
    !validChord(chord) ||
    typeof event.key !== "string" ||
    isCompositionKey(event) ||
    event.defaultPrevented ||
    event.getModifierState?.("AltGraph")
  )
    return false;
  const key = event.key === " " ? "space" : event.key.toLowerCase();
  return (
    key === chord.key &&
    event.ctrlKey === chord.ctrl &&
    event.metaKey === chord.meta &&
    event.altKey === chord.alt &&
    event.shiftKey === chord.shift
  );
}
