import { expect, it } from "vitest";
import { applyUiAccent } from "./workspace-accent";
import {
  decodeWorkspaceSettings,
  validateSettingsChange,
} from "./workspace-settings";

const fixture = () => ({
  version: 12,
  token: "00000000-0000-4000-8000-000000000001",
  terminal_presets: ["classic", "midnight", "paper"].map((id) => ({
    id,
    colors: {
      ansi: Array.from({ length: 16 }, () => [100, 100, 100]),
      foreground: [250, 250, 250],
      background: [20, 20, 20],
      cursor: [250, 250, 250],
    },
  })),
  values: Object.fromEntries(
    Object.entries({
      font_size: 14,
      font_family: "monospace",
      font_fallback: "monospace",
      font_ligatures: false,
      line_height_percent: 100,
      letter_spacing: 0,
      cursor_blink: false,
      copy_on_select: false,
      right_click_paste: false,
      ui_appearance: "light",
      ui_accent: "cyan",
      terminal_preset: "classic",
      history_limit: 100000,
      scroll_on_input: true,
      scroll_on_output: false,
      mac_option_as_meta: false,
      backspace_send: "del",
      delete_send: "csi",
      term_type: "xterm-256color",
    }).map(([key, value]) => [key, { value, source: "default" }]),
  ),
});
it("v12 requires a fixed accent with user provenance; v11 remains compatible", () => {
  const value = fixture();
  expect(decodeWorkspaceSettings(value).values.ui_accent).toEqual({
    value: "cyan",
    source: "default",
  });
  for (const accent of ["blue", "green", "amber", "red"]) {
    value.values.ui_accent = { value: accent, source: "user" };
    expect(decodeWorkspaceSettings(value).values.ui_accent?.value).toBe(accent);
  }
  for (const setting of [
    { value: "purple", source: "user" },
    { value: "var(--red-9)", source: "user" },
    { value: "red", source: "host" },
  ])
    expect(() =>
      decodeWorkspaceSettings({
        ...value,
        values: { ...value.values, ui_accent: setting },
      }),
    ).toThrow();
  const { ui_accent: _accent, ...legacy } = value.values;
  expect(() => decodeWorkspaceSettings({ ...value, values: legacy })).toThrow();
  expect(
    decodeWorkspaceSettings({ ...value, version: 11, values: legacy }).values
      .ui_accent,
  ).toBeUndefined();
  expect(() => decodeWorkspaceSettings({ ...value, version: 11 })).toThrow();
  expect(() => decodeWorkspaceSettings({ ...value, version: 13 })).toThrow();
});
it("set/reset admit only exact accent scalars", () => {
  expect(validateSettingsChange({ field: "ui_accent", value: "blue" })).toEqual(
    { field: "ui_accent", value: "blue" },
  );
  expect(validateSettingsChange({ field: "reset_ui_accent" })).toEqual({
    field: "reset_ui_accent",
  });
  for (const change of [
    { field: "ui_accent", value: "toString" },
    { field: "ui_accent", value: "#123456" },
    { field: "reset_ui_accent", value: 0 },
  ])
    expect(() => validateSettingsChange(change)).toThrow();
});
it("UI projection uses a fixed attribute, refuses CSS input and abort", () => {
  const attributes = new Map();
  const doc = {
    documentElement: {
      setAttribute: (key: string, value: string) => attributes.set(key, value),
    },
  } as unknown as Document;
  const life = new AbortController();
  expect(applyUiAccent(doc, "blue", life.signal)).toBe(true);
  expect(attributes.get("data-ui-accent")).toBe("blue");
  expect(applyUiAccent(doc, "red;display:none", life.signal)).toBe(false);
  life.abort();
  expect(applyUiAccent(doc, "green", life.signal)).toBe(false);
  expect(attributes.get("data-ui-accent")).toBe("blue");
});
