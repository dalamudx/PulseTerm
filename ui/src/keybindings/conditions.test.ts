import { expect, it } from "vitest";
import {
  type BindingCondition,
  type BindingContext,
  bindingConditionMatches,
  bindingConditionsOverlap,
  bindingContextFields,
  conditionExcludesTerminal,
  parseBindingCondition,
} from "./conditions";

function context(flags: number): BindingContext {
  return Object.fromEntries(
    bindingContextFields.map((field, index) => [
      field,
      !!(flags & (1 << index)),
    ]),
  ) as BindingContext;
}
it("parses fixed conjunction/negation as immutable metadata, without an evaluator", () => {
  const parsed = parseBindingCondition("terminalFocus && !broadcast");
  expect(parsed).toEqual([
    { field: "terminalFocus", required: true },
    { field: "broadcast", required: false },
  ]);
  expect(Object.isFrozen(parsed)).toBe(true);
  expect(Object.isFrozen(parsed[0])).toBe(true);
});
it("unknown expressions, access, duplicates and unbounded payloads are rejected", () => {
  for (const value of [
    null,
    {},
    1,
    "__proto__",
    "window.focus()",
    "terminalFocus || broadcast",
    "terminalFocus &&",
    "terminalFocus && !terminalFocus",
    "terminalFocus && terminalFocus",
    "! terminalFocus",
    "!!terminalFocus",
    "terminalFocus === true",
    "terminalFocus && permission",
    "a".repeat(129),
  ])
    expect(() => parseBindingCondition(value)).toThrow();
});
it("all32 boolean contexts match the declared literals, and malformed flags never match", () => {
  const condition = parseBindingCondition("terminalFocus && !broadcast");
  for (let flags = 0; flags < 32; flags++) {
    const c = context(flags);
    expect(bindingConditionMatches(condition, c)).toBe(
      c.terminalFocus && !c.broadcast,
    );
  }
  expect(
    bindingConditionMatches(parseBindingCondition(""), {
      ...context(0),
      broadcast: "false",
    } as unknown as BindingContext),
  ).toBe(false);
});
it("overlap exactly matches exhaustive boolean context enumeration for conjunctions", () => {
  const conditions = [
    "",
    "terminalFocus",
    "!terminalFocus",
    "terminalFocus && !broadcast",
    "workspaceFocus && !editableFocus",
    "editableFocus",
    "modalOpen && broadcast",
    "!modalOpen",
  ].map(parseBindingCondition);
  for (const a of conditions)
    for (const b of conditions) {
      const expected = Array.from({ length: 32 }, (_, flags) =>
        context(flags),
      ).some(
        (c) => bindingConditionMatches(a, c) && bindingConditionMatches(b, c),
      );
      expect(bindingConditionsOverlap(a, b)).toBe(expected);
    }
});
it("terminal exclusion must be explicit, not inferred from a context name", () => {
  expect(
    conditionExcludesTerminal(parseBindingCondition("workspaceFocus")),
  ).toBe(false);
  expect(
    conditionExcludesTerminal(parseBindingCondition("editableFocus")),
  ).toBe(false);
  expect(
    conditionExcludesTerminal(
      parseBindingCondition("!terminalFocus && workspaceFocus"),
    ),
  ).toBe(true);
});
it("forged runtime condition fields/flags cannot bypass syntax validation", () => {
  for (const value of [
    [{ field: "permission", required: true }],
    [{ field: "terminalFocus", required: "yes" }],
    [
      { field: "terminalFocus", required: true },
      { field: "terminalFocus", required: false },
    ],
  ]) {
    const condition = value as unknown as BindingCondition;
    expect(() => bindingConditionMatches(condition, context(0))).toThrow();
    expect(() => bindingConditionsOverlap(condition, [])).toThrow();
  }
});
