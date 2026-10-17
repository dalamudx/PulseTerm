import { expect, it } from "vitest";
import { operatorLigatures } from "./operator-ligatures";

it("joins bounded ASCII operators without claiming UTF-16 indices are columns", () => {
  const text = "中😀e\u0301 => !== === -> != <= >= <=> && || :: := ...";
  const joins = operatorLigatures(text);
  expect(joins.map(([start, end]) => text.slice(start, end))).toEqual([
    "=>",
    "!==",
    "===",
    "->",
    "!=",
    "<=",
    ">=",
    "<=>",
    "&&",
    "||",
    "::",
    ":=",
    "...",
  ]);
  expect(joins[0]?.[0]).toBe(text.indexOf("=>"));
  expect(operatorLigatures("== ".repeat(1024))).toHaveLength(64);
  expect(operatorLigatures("untrusted ordinary text 中文 😀")).toEqual([]);
});
