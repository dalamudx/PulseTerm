import { expect, it } from "vitest";
import { workspacePaletteOnlyCommandIds } from "../workspace-command-catalog";
import { compileKeybindingDefinitions } from "./definitions";

it("does not admit palette navigation IDs into the persisted keymap", () => {
  for (const action of workspacePaletteOnlyCommandIds) {
    expect(() =>
      compileKeybindingDefinitions([
        { action, keys: "ctrl+shift+s", when: "!modalOpen" },
      ]),
    ).toThrow("keymap.invalid_binding");
  }
});

it("inherited fields and getters are not backend JSON projection data", () => {
  let called = false;
  const getter = {
    keys: "ctrl+shift+s",
    get action() {
      called = true;
      return "settings";
    },
  };
  expect(() => compileKeybindingDefinitions([getter])).toThrow();
  expect(called).toBe(false);
  expect(() =>
    compileKeybindingDefinitions([
      Object.create({ action: "settings", keys: "ctrl+shift+s" }),
    ]),
  ).toThrow();
});
const binding = (extra: object = {}) => ({
  action: "settings",
  keys: "ctrl+shift+s",
  when: "workspaceFocus && !editableFocus && !modalOpen",
  ...extra,
});
it("compiles immutable bounded current UI actions without execution handlers", () => {
  const compiled = compileKeybindingDefinitions([
    binding(),
    binding({ action: "palette", keys: "ctrl+shift+p" }),
  ]);
  expect(compiled.map((b) => b.action)).toEqual(["settings", "palette"]);
  expect(Object.isFrozen(compiled)).toBe(true);
  expect(Object.isFrozen(compiled[0])).toBe(true);
  expect("run" in (compiled[0] ?? {})).toBe(false);
});
it("unknown or malformed source rejects the whole configuration", () => {
  for (const input of [
    null,
    {},
    [binding(), binding({ action: "exec" })],
    [binding({ path: "/bin/sh" })],
    [binding({ when: null })],
    [binding({ keys: "arbitrary()" })],
    Array.from({ length: 17 }, () => binding()),
  ])
    expect(() => compileKeybindingDefinitions(input)).toThrow();
});
it("ordinary ctrl+k prefix cannot capture terminal context", () => {
  expect(() =>
    compileKeybindingDefinitions([
      binding({ keys: "ctrl+k ctrl+s", when: "terminalFocus && !broadcast" }),
    ]),
  ).toThrow("terminal_key_reserved");
  const parsed = compileKeybindingDefinitions([
    binding({
      keys: "ctrl+k ctrl+s",
      when: "workspaceFocus && !terminalFocus && !editableFocus",
    }),
  ]);
  expect(parsed).toHaveLength(1);
});
it("safe modified two-step sequences can be terminal candidates but are not permissions", () => {
  expect(
    compileKeybindingDefinitions([
      binding({
        keys: "ctrl+shift+k ctrl+shift+s",
        when: "terminalFocus && !broadcast",
      }),
    ]),
  ).toHaveLength(1);
});
it("clipboard and reload gestures remain owned by their original routes", () => {
  for (const keys of ["ctrl+shift+c", "meta+shift+v", "ctrl+shift+r"])
    expect(() => compileKeybindingDefinitions([binding({ keys })])).toThrow(
      "product_key_reserved",
    );
});
it("current palette chord cannot be repurposed into another UI action", () => {
  expect(() =>
    compileKeybindingDefinitions([binding({ keys: "ctrl+shift+p" })]),
  ).toThrow("product_key_reserved");
  expect(
    compileKeybindingDefinitions([
      binding({ action: "palette", keys: "ctrl+shift+p" }),
    ]),
  ).toHaveLength(1);
});
it("exact and prefix bindings collide when their contexts overlap", () => {
  for (const keys of ["ctrl+shift+k", "ctrl+shift+k ctrl+shift+s"])
    expect(() =>
      compileKeybindingDefinitions([
        binding({ keys: "ctrl+shift+k" }),
        binding({ action: "hosts", keys }),
      ]),
    ).toThrow("binding_conflict");
});
it("contradictory contexts permit independent bindings with the same key", () => {
  expect(
    compileKeybindingDefinitions([
      binding({ when: "terminalFocus" }),
      binding({ action: "hosts", when: "!terminalFocus" }),
    ]),
  ).toHaveLength(2);
});
it("different complete sequences sharing a prefix do not collide", () => {
  expect(
    compileKeybindingDefinitions([
      binding({ keys: "ctrl+shift+k ctrl+shift+s" }),
      binding({ action: "hosts", keys: "ctrl+shift+k ctrl+shift+h" }),
    ]),
  ).toHaveLength(2);
});
