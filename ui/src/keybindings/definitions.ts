import {
  type WorkspaceKeybindingCommandId,
  workspaceKeybindingCommandIds,
} from "../workspace-command-catalog";
import {
  type ChordSequence,
  chordSequencesConflict,
  isModifiedAppChord,
  parseChordSequence,
} from "./chords";
import {
  type BindingCondition,
  bindingConditionsOverlap,
  conditionExcludesTerminal,
  parseBindingCondition,
} from "./conditions";
export type KeybindingAction = WorkspaceKeybindingCommandId | "palette";
export interface KeybindingDefinition {
  readonly action: KeybindingAction;
  readonly sequence: ChordSequence;
  readonly when: BindingCondition;
}
const actions: readonly string[] = [
  ...workspaceKeybindingCommandIds,
  "palette",
];
function knownAction(value: unknown): value is KeybindingAction {
  return typeof value === "string" && actions.includes(value);
}
/** Backend-projected UI metadata only: cannot invoke arbitrary Capability IDs or commands. */
export function compileKeybindingDefinitions(
  value: unknown,
): readonly KeybindingDefinition[] {
  if (!Array.isArray(value) || value.length > 16)
    throw new Error("keymap.invalid_bindings");
  const bindings = value.map((value: unknown) => {
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw new Error("keymap.invalid_binding");
    const prototype = Object.getPrototypeOf(value);
    if (
      (prototype !== Object.prototype && prototype !== null) ||
      !Object.hasOwn(value, "action") ||
      !Object.hasOwn(value, "keys") ||
      Object.values(Object.getOwnPropertyDescriptors(value)).some(
        (descriptor) => !("value" in descriptor),
      )
    )
      throw new Error("keymap.invalid_binding");
    const record = value as Record<string, unknown>;
    if (
      Object.keys(record).some(
        (key) => key !== "action" && key !== "keys" && key !== "when",
      ) ||
      !knownAction(record.action)
    )
      throw new Error("keymap.invalid_binding");
    const sequence = parseChordSequence(record.keys),
      when = parseBindingCondition(
        record.when === undefined ? "" : record.when,
      );
    // Ordinary shell keys (including ctrl+k prefixes) may only be selected in an
    // explicitly nonterminal context; no inferred editable-focus authorization.
    if (
      !conditionExcludesTerminal(when) &&
      sequence.some((chord) => !isModifiedAppChord(chord))
    )
      throw new Error("keymap.terminal_key_reserved");
    // Preserve the already-owned clipboard/reload gestures. Configurable ownership
    // of those product actions requires a separate registration, not interception.
    if (
      sequence.some(
        (chord) =>
          (chord.ctrl || chord.meta) &&
          chord.shift &&
          !chord.alt &&
          ["c", "v", "r"].includes(chord.key),
      )
    )
      throw new Error("keymap.product_key_reserved");
    if (
      sequence.some(
        (chord) =>
          chord.shift &&
          !chord.alt &&
          (chord.ctrl || chord.meta) &&
          chord.key === "p",
      ) &&
      record.action !== "palette"
    )
      throw new Error("keymap.product_key_reserved");
    return Object.freeze({ action: record.action, sequence, when });
  });
  for (let i = 0; i < bindings.length; i++)
    for (let j = i + 1; j < bindings.length; j++) {
      const a = bindings[i],
        b = bindings[j];
      if (
        a &&
        b &&
        bindingConditionsOverlap(a.when, b.when) &&
        chordSequencesConflict(a.sequence, b.sequence)
      )
        throw new Error("keymap.binding_conflict");
    }
  return Object.freeze(bindings);
}
