import { expect, it } from "vitest";
import { classifyKeymapFocus } from "./focus-context";

it("xterm helper textarea keeps terminal conditions enabled", () => {
  expect(classifyKeymapFocus({ closest: () => ({}) })).toEqual({
    terminalFocus: true,
    editableFocus: false,
  });
});
it("ordinary form input/textarea/select and contenteditable remain protected", () => {
  expect(
    classifyKeymapFocus({
      closest: (selector) => (selector === "#workspace-terminal" ? null : {}),
    }),
  ).toEqual({ terminalFocus: false, editableFocus: true });
});
it("workspace controls and absent focus are not falsely classified as terminal", () => {
  expect(classifyKeymapFocus({ closest: () => null })).toEqual({
    terminalFocus: false,
    editableFocus: false,
  });
  expect(classifyKeymapFocus(null)).toEqual({
    terminalFocus: false,
    editableFocus: false,
  });
});
