import { expect, it } from "vitest";
import { displayPreferences } from "./features/terminal/display-preferences";
import { terminalFontFamily } from "./features/terminal/terminal-fonts";
import {
  decodeWorkspaceSettings,
  validateSettingsChange,
} from "./workspace-settings";

const token = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const modern = {
  version: 3,
  token,
  values: {
    font_size: { value: 14, source: "default" },
    font_family: { value: "JetBrains Mono", source: "user" },
    font_fallback: { value: "WenQuanYi Zen Hei Mono", source: "user" },
    font_ligatures: { value: false, source: "user" },
    line_height_percent: { value: 100, source: "default" },
    letter_spacing: { value: 0, source: "default" },
    cursor_blink: { value: false, source: "default" },
    copy_on_select: { value: false, source: "default" },
    right_click_paste: { value: false, source: "default" },
  },
};
it("v3 requires safe fallback and boolean shaping with independent sources; v2 rejects injected fields", () => {
  const decoded = decodeWorkspaceSettings(modern);
  expect(decoded.values.font_ligatures).toEqual({
    value: false,
    source: "user",
  });
  expect(decoded.values.font_fallback?.value).toBe("WenQuanYi Zen Hei Mono");
  expect(() => decodeWorkspaceSettings({ ...modern, version: 2 })).toThrow();
  expect(() =>
    decodeWorkspaceSettings({
      ...modern,
      values: {
        ...modern.values,
        font_ligatures: { value: 1, source: "user" },
      },
    }),
  ).toThrow();
  expect(() =>
    validateSettingsChange({ field: "font_fallback", value: 'mono",url(x)' }),
  ).toThrow();
  expect(
    validateSettingsChange({ field: "font_ligatures", value: true }),
  ).toEqual({ field: "font_ligatures", value: true });
  expect(
    validateSettingsChange({ field: "font_ligatures", value: false }),
  ).toEqual({ field: "font_ligatures", value: false });
  const draw = displayPreferences({
    fontSize: 14,
    fontFamily: "JetBrains Mono",
    fontFallback: "WenQuanYi Zen Hei Mono",
    fontLigatures: false,
    cursorBlink: false,
  });
  expect(terminalFontFamily(draw)).toBe(
    '"JetBrains Mono", "WenQuanYi Zen Hei Mono", monospace',
  );
  expect(() =>
    displayPreferences({ ...draw, fontFallback: "monospace,serif" }),
  ).toThrow();
});
