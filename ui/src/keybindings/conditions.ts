export const bindingContextFields = [
  "workspaceFocus",
  "terminalFocus",
  "editableFocus",
  "modalOpen",
  "broadcast",
] as const;
export type BindingContextField = (typeof bindingContextFields)[number];
export type BindingContext = Readonly<Record<BindingContextField, boolean>>;
export interface ConditionTerm {
  readonly field: BindingContextField;
  readonly required: boolean;
}
export type BindingCondition = readonly ConditionTerm[];
function field(value: string): value is BindingContextField {
  return bindingContextFields.some((name) => name === value);
}
/** A small conjunction grammar, not JavaScript or backend execution authorization. */
export function parseBindingCondition(value: unknown): BindingCondition {
  if (typeof value !== "string" || value.length > 128)
    throw new Error("keymap.invalid_condition");
  if (!value.trim()) return Object.freeze([]);
  const seen = new Set<BindingContextField>();
  const terms = value.split("&&");
  if (terms.length > bindingContextFields.length)
    throw new Error("keymap.invalid_condition");
  return Object.freeze(
    terms.map((value) => {
      const term = value.trim(),
        required = !term.startsWith("!"),
        name = required ? term : term.slice(1);
      if (!field(name) || seen.has(name))
        throw new Error("keymap.invalid_condition");
      seen.add(name);
      return Object.freeze({ field: name, required });
    }),
  );
}
function validate(condition: BindingCondition): void {
  if (condition.length > bindingContextFields.length)
    throw new Error("keymap.invalid_condition");
  const seen = new Set<BindingContextField>();
  for (const term of condition) {
    if (
      !term ||
      !field(term.field) ||
      typeof term.required !== "boolean" ||
      seen.has(term.field)
    )
      throw new Error("keymap.invalid_condition");
    seen.add(term.field);
  }
}
export function bindingConditionMatches(
  condition: BindingCondition,
  context: BindingContext,
): boolean {
  validate(condition);
  return (
    bindingContextFields.every(
      (field) => typeof context[field] === "boolean",
    ) && condition.every((term) => context[term.field] === term.required)
  );
}
/** Conservative conflict detection; fields are independent, no inferred focus privileges. */
export function bindingConditionsOverlap(
  left: BindingCondition,
  right: BindingCondition,
): boolean {
  validate(left);
  validate(right);
  return !left.some((a) =>
    right.some((b) => a.field === b.field && a.required !== b.required),
  );
}
export function conditionExcludesTerminal(
  condition: BindingCondition,
): boolean {
  validate(condition);
  return condition.some(
    (term) => term.field === "terminalFocus" && !term.required,
  );
}
