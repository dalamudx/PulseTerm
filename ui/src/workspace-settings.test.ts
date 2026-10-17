import { expect, it, vi } from "vitest";
import {
  decodeWorkspaceSettings,
  readWorkspaceSettings,
  saveWorkspaceSettings,
  validateSettingsChange,
} from "./workspace-settings";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const projection = () => ({
  version: 1,
  token: nonce,
  values: {
    font_size: { value: 14, source: "default" },
    font_family: { value: "monospace", source: "user" },
    cursor_blink: { value: false, source: "user" },
    copy_on_select: { value: false, source: "default" },
    right_click_paste: { value: false, source: "default" },
  },
});
it("bounded exact fields and per-field provenance, false not missing", () => {
  const source = projection(),
    r = decodeWorkspaceSettings(source);
  expect(r.values.cursor_blink.value).toBe(false);
  expect(r.values.font_family.source).toBe("user");
  source.values.font_size.value = 30;
  expect(r.values.font_size.value).toBe(14);
  expect(Object.isFrozen(r.values)).toBe(true);
});
it("future/extra/missing/unsafe/fontsize/source substitutions rejected", () => {
  for (const mutate of [
    (v: ReturnType<typeof projection>) => ({ ...v, version: 2 }),
    (v: ReturnType<typeof projection>) => ({ ...v, path: "/tmp" }),
    (v: ReturnType<typeof projection>) => ({ ...v, token: "foreign" }),
    (v: ReturnType<typeof projection>) => ({
      ...v,
      values: { ...v.values, font_size: { value: 73, source: "default" } },
    }),
    (v: ReturnType<typeof projection>) => ({
      ...v,
      values: {
        ...v.values,
        font_family: { value: "url(foo)", source: "user" },
      },
    }),
    (v: ReturnType<typeof projection>) => ({
      ...v,
      values: { ...v.values, cursor_blink: { value: 0, source: "user" } },
    }),
    (v: ReturnType<typeof projection>) => ({
      ...v,
      values: { ...v.values, font_size: { value: 14, source: "host" } },
    }),
  ])
    expect(() => decodeWorkspaceSettings(mutate(projection()))).toThrow(
      "settings.invalid",
    );
});
it("only explicit three scalar set/reset mutations", () => {
  for (const value of [
    { field: "font_size", value: 6 },
    { field: "font_size", value: 72 },
    { field: "font_family", value: "等宽字体" },
    { field: "cursor_blink", value: false },
    { field: "reset_font_size" },
    { field: "reset_font_family" },
    { field: "reset_cursor_blink" },
  ])
    expect(validateSettingsChange(value)).toEqual(value);
  for (const value of [
    { field: "cwd", value: "/tmp" },
    { field: "font_family", value: "mono; color:red" },
    { field: "font_size", value: 12.5 },
    { field: "cursor_blink", value: 1 },
    { field: "reset_font_size", value: 0 },
  ])
    expect(() => validateSettingsChange(value)).toThrow();
});
it("page cancellation does not retry write or accept late receipt", async () => {
  const scope = new AbortController();
  let reply!: (v: unknown) => void;
  const rpc = vi.fn(
    () =>
      new Promise<unknown>((resolve) => {
        reply = resolve;
      }),
  );
  const pending = saveWorkspaceSettings(
    nonce,
    nonce,
    { field: "font_size", value: 18 },
    scope.signal,
    rpc,
  );
  scope.abort();
  await expect(pending).rejects.toThrow("settings.cancelled");
  reply(projection());
  expect(rpc).toHaveBeenCalledOnce();
});
it("pre-cancelled scope never invokes and malformed server reply fails closed", async () => {
  const scope = new AbortController();
  scope.abort();
  const rpc = vi.fn(async () => projection());
  await expect(readWorkspaceSettings(nonce, scope.signal, rpc)).rejects.toThrow(
    "settings.cancelled",
  );
  expect(rpc).not.toHaveBeenCalled();
  await expect(
    readWorkspaceSettings(nonce, new AbortController().signal, async () => ({
      version: 2,
    })),
  ).rejects.toThrow("settings.invalid");
});

it("v2 cell metrics use independent sources; legacy v1 cannot smuggle metrics", () => {
  const modern = {
    ...projection(),
    version: 2,
    values: {
      ...projection().values,
      line_height_percent: { value: 150, source: "user" },
      letter_spacing: { value: 0, source: "user" },
    },
  };
  const decoded = decodeWorkspaceSettings(modern);
  expect(decoded.values.line_height_percent).toEqual({
    value: 150,
    source: "user",
  });
  expect(decoded.values.letter_spacing).toEqual({ value: 0, source: "user" });
  expect(Object.isFrozen(decoded.values.letter_spacing)).toBe(true);
  expect(() => decodeWorkspaceSettings({ ...modern, version: 1 })).toThrow();
  expect(() =>
    decodeWorkspaceSettings({ ...projection(), version: 2 }),
  ).toThrow();
  for (const [field, badValue] of [
    ["line_height_percent", 99],
    ["line_height_percent", 301],
    ["line_height_percent", 150.5],
    ["line_height_percent", Infinity],
    ["letter_spacing", -1],
    ["letter_spacing", 9],
    ["letter_spacing", 0.5],
  ] as const) {
    expect(() =>
      decodeWorkspaceSettings({
        ...modern,
        values: {
          ...modern.values,
          [field]: { value: badValue, source: "user" },
        },
      }),
    ).toThrow();
    expect(() => validateSettingsChange({ field, value: badValue })).toThrow();
  }
  expect(validateSettingsChange({ field: "letter_spacing", value: 0 })).toEqual(
    { field: "letter_spacing", value: 0 },
  );
  expect(
    validateSettingsChange({ field: "reset_line_height_percent" }),
  ).toEqual({ field: "reset_line_height_percent" });
});

it("explicit operator-ligature settings reach the real RPC with bounded boolean type", async () => {
  const rpc = vi.fn(async () => projection());
  const signal = new AbortController().signal;
  await saveWorkspaceSettings(
    nonce,
    nonce,
    { field: "font_ligatures", value: true },
    signal,
    rpc,
  );
  expect(rpc).toHaveBeenLastCalledWith("workspace_settings_save", {
    nonce,
    token: nonce,
    change: { field: "font_ligatures", value: true },
  });
  for (const value of ["true", 1, null, undefined]) {
    expect(() =>
      validateSettingsChange({ field: "font_ligatures", value }),
    ).toThrow();
  }
  expect(rpc).toHaveBeenCalledOnce();
});

it("v4 adds a closed UI preference while retaining v1-v3 decoders", () => {
  const modern = {
    ...projection(),
    version: 4,
    values: {
      ...projection().values,
      line_height_percent: { value: 100, source: "default" },
      letter_spacing: { value: 0, source: "default" },
      font_fallback: { value: "monospace", source: "default" },
      font_ligatures: { value: false, source: "default" },
      ui_appearance: { value: "system", source: "user" },
    },
  };
  expect(decodeWorkspaceSettings(modern).values.ui_appearance).toEqual({
    value: "system",
    source: "user",
  });
  for (const version of [1, 2, 3, 5])
    expect(() => decodeWorkspaceSettings({ ...modern, version })).toThrow();
  for (const value of ["auto", "url(x)", "DARK", true, null]) {
    expect(() =>
      decodeWorkspaceSettings({
        ...modern,
        values: { ...modern.values, ui_appearance: { value, source: "user" } },
      }),
    ).toThrow();
    expect(() =>
      validateSettingsChange({ field: "ui_appearance", value }),
    ).toThrow();
  }
  expect(() =>
    validateSettingsChange({ field: "reset_ui_appearance", value: "light" }),
  ).toThrow();
  expect(
    validateSettingsChange({ field: "ui_appearance", value: "dark" }),
  ).toEqual({
    field: "ui_appearance",
    value: "dark",
  });
  expect(validateSettingsChange({ field: "reset_ui_appearance" })).toEqual({
    field: "reset_ui_appearance",
  });
});

it("input scroll change accepts explicit booleans and rejects coercion", () => {
  expect(
    validateSettingsChange({ field: "scroll_on_input", value: false }),
  ).toEqual({ field: "scroll_on_input", value: false });
  expect(validateSettingsChange({ field: "reset_scroll_on_input" })).toEqual({
    field: "reset_scroll_on_input",
  });
  for (const value of [0, "false", null, undefined])
    expect(() =>
      validateSettingsChange({ field: "scroll_on_input", value }),
    ).toThrow("settings.invalid");
  const old = decodeWorkspaceSettings(projection());
  expect(old.values.scroll_on_input).toBeUndefined();
});
