import { expect, it } from "vitest";
import { isCommandPaletteShortcut } from "../workspace-command-catalog";

it("shared matcher preserves all768 combinations of the previously real-window-validated palette predicate", () => {
  for (let mods = 0; mods < 16; mods++)
    for (let state = 0; state < 8; state++)
      for (const key of ["p", "P", "q", "ArrowUp", "", "Meta"]) {
        const event = {
          key,
          ctrlKey: !!(mods & 1),
          metaKey: !!(mods & 2),
          altKey: !!(mods & 4),
          shiftKey: !!(mods & 8),
          repeat: !!(state & 1),
          isComposing: !!(state & 2),
          defaultPrevented: !!(state & 4),
        };
        const legacy =
          !event.defaultPrevented &&
          !event.repeat &&
          !event.isComposing &&
          !event.altKey &&
          (event.ctrlKey || event.metaKey) &&
          event.shiftKey &&
          event.key.toLowerCase() === "p";
        expect(isCommandPaletteShortcut(event)).toBe(legacy);
      }
});
