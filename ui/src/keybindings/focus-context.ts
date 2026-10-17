export interface KeymapFocusTarget {
  closest(selector: string): unknown;
}
/** xterm's helper textarea is terminal input, not an application settings form. */
export function classifyKeymapFocus(
  target: KeymapFocusTarget | null,
): Readonly<{ terminalFocus: boolean; editableFocus: boolean }> {
  const terminalFocus = !!target?.closest("#workspace-terminal");
  const editableFocus =
    !terminalFocus &&
    !!target?.closest(
      "input,textarea,select,[contenteditable=true],[contenteditable='']",
    );
  return Object.freeze({ terminalFocus, editableFocus });
}
