export const BUNDLED_TERMINAL_FONT = "JetBrains Mono";
export const TERMINAL_FONT_PRESETS = Object.freeze([
  Object.freeze({ id: "system", label: "系统等宽", family: "monospace" }),
  Object.freeze({
    id: "jetbrains",
    label: "JetBrains Mono",
    family: BUNDLED_TERMINAL_FONT,
  }),
]);
