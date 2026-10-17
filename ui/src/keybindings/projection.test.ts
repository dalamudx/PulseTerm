import { expect, it } from "vitest";
import { workspaceKeybindingCommandIds } from "../workspace-command-catalog";
import { decodeKeymapProjection } from "./projection";

function projection() {
  return {
    version: 1,
    bindings: [
      {
        action: "find",
        keys: "ctrl+shift+f",
        when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
      },
      {
        action: "find",
        keys: "meta+shift+f",
        when: "workspaceFocus && terminalFocus && !editableFocus && !modalOpen && !broadcast",
      },
      {
        action: "palette",
        keys: "ctrl+shift+p",
        when: "workspaceFocus && !editableFocus && !modalOpen",
      },
      {
        action: "palette",
        keys: "meta+shift+p",
        when: "workspaceFocus && !editableFocus && !modalOpen",
      },
    ],
    sources: [...workspaceKeybindingCommandIds, "palette"].map((action) => ({
      action,
      source: "default",
      disabled: action !== "palette" && action !== "find",
    })),
  };
}
it("decodes immutable metadata for nine action sources without handlers", () => {
  const result = decodeKeymapProjection(projection());
  expect(result.bindings).toHaveLength(4);
  expect(result.sources).toHaveLength(9);
  expect(Object.isFrozen(result)).toBe(true);
  expect(Object.isFrozen(result.sources)).toBe(true);
  expect("run" in result).toBe(false);
});
it("user empty keys can disable palette while preserving the UI action metadata", () => {
  const value = projection();
  value.bindings = value.bindings.filter(
    (binding) => binding.action !== "palette",
  );
  value.sources = value.sources.map((source) =>
    source.action === "palette"
      ? { ...source, source: "user", disabled: true }
      : source,
  );
  const decoded = decodeKeymapProjection(value);
  expect(decoded.bindings).toHaveLength(2);
  expect(
    decoded.sources.find((source) => source.action === "palette")?.source,
  ).toBe("user");
});
it("version, exact fields, arrays and action set are not relaxed", () => {
  for (const value of [
    null,
    {},
    { ...projection(), version: 2 },
    { ...projection(), path: "/bin/sh" },
    { ...projection(), bindings: {} },
    { ...projection(), sources: [] },
    {
      ...projection(),
      sources: Array(workspaceKeybindingCommandIds.length + 1).fill(
        projection().sources[0],
      ),
    },
    {
      ...projection(),
      sources: projection().sources.map((source) => ({
        ...source,
        action: "exec",
      })),
    },
  ])
    expect(() => decodeKeymapProjection(value)).toThrow();
});
it("source flags must agree with the effective aliases, not caller assertions", () => {
  const base = projection();
  for (const change of [
    { action: "palette", source: "default", disabled: true },
    { action: "settings", source: "user", disabled: false },
    { action: "settings", source: "default", disabled: "true" },
    { action: "settings", source: "remote", disabled: true },
  ])
    expect(() =>
      decodeKeymapProjection({
        ...base,
        sources: base.sources.map((source) =>
          source.action === change.action ? change : source,
        ),
      }),
    ).toThrow();
});
it("binding compile still rejects conflicting sequences and Shell key interception", () => {
  for (const keys of ["ctrl+p", "ctrl+k ctrl+s"]) {
    const value = projection();
    value.bindings = [{ action: "settings", keys, when: "terminalFocus" }];
    expect(() => decodeKeymapProjection(value)).toThrow();
  }
  const value = projection();
  value.bindings.push({
    ...value.bindings[0],
    action: "palette",
    keys: "ctrl+shift+p",
    when: "",
  });
  expect(() => decodeKeymapProjection(value)).toThrow();
});
it("only plain JSON data is admitted and accessors are never executed", () => {
  let invoked = false;
  const value = {
    ...projection(),
    get version() {
      invoked = true;
      return 1;
    },
  };
  expect(() => decodeKeymapProjection(value)).toThrow();
  expect(invoked).toBe(false);
  expect(() => decodeKeymapProjection(Object.create(projection()))).toThrow();
});
it("unknown binding or source fields invalidate the whole receipt", () => {
  const value = projection();
  expect(() =>
    decodeKeymapProjection({
      ...value,
      bindings: value.bindings.map((binding) => ({
        ...binding,
        run: "command",
      })),
    }),
  ).toThrow();
  expect(() =>
    decodeKeymapProjection({
      ...value,
      sources: value.sources.map((source) => ({ ...source, permission: true })),
    }),
  ).toThrow();
});
