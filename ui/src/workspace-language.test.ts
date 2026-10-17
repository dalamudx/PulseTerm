import { expect, it } from "vitest";
import {
  applyUiLanguage,
  englishMessages,
  isUiLanguage,
  message,
  translate,
} from "./workspace-language";
import {
  decodeWorkspaceSettings,
  validateSettingsChange,
} from "./workspace-settings";
import { terminalPresetIds } from "./workspace-terminal-preset";

function projection() {
  const setting = <T>(value: T) => ({ value, source: "default" });
  return {
    version: 15,
    token: "11111111-1111-4111-8111-111111111111",
    terminal_presets: terminalPresetIds.map((id) => ({
      id,
      colors: {
        foreground: [230, 230, 230],
        background: [0, 0, 0],
        cursor: [230, 230, 230],
        ansi: Array.from({ length: 16 }, () => [120, 120, 120]),
      },
    })),
    values: {
      font_size: setting(14),
      font_family: setting("monospace"),
      font_fallback: setting("monospace"),
      font_ligatures: setting(false),
      line_height_percent: setting(100),
      letter_spacing: setting(0),
      cursor_blink: setting(false),
      scroll_on_input: setting(true),
      scroll_on_output: setting(false),
      mac_option_as_meta: setting(false),
      backspace_send: setting("del"),
      delete_send: setting("csi"),
      term_type: setting("xterm-256color"),
      copy_on_select: setting(false),
      right_click_paste: setting(false),
      ui_appearance: setting("light"),
      ui_accent: setting("cyan"),
      ui_language: setting("en"),
      terminal_preset: setting("classic"),
      terminal_theme: setting({
        mode: "fixed",
        light: "paper",
        dark: "midnight",
      }),
      history_limit: setting(100000),
    },
  };
}
it("v15 requires a closed language with provenance, v14 keeps its exact old shape", () => {
  const raw = projection();
  expect(decodeWorkspaceSettings(raw).values.ui_language).toEqual({
    value: "en",
    source: "default",
  });
  expect(() => decodeWorkspaceSettings({ ...raw, version: 14 })).toThrow();
  const { ui_language: _language, ...legacy } = raw.values;
  expect(
    decodeWorkspaceSettings({ ...raw, version: 14, values: legacy }).values
      .ui_language,
  ).toBeUndefined();
  expect(() => decodeWorkspaceSettings({ ...raw, values: legacy })).toThrow();
  for (const language of ["en-US", "zh", "EN", "../../en", true, null]) {
    expect(isUiLanguage(language)).toBe(false);
    expect(() =>
      decodeWorkspaceSettings({
        ...raw,
        values: {
          ...raw.values,
          ui_language: { value: language, source: "user" },
        },
      }),
    ).toThrow();
    expect(() =>
      validateSettingsChange({ field: "ui_language", value: language }),
    ).toThrow();
  }
  expect(
    validateSettingsChange({ field: "ui_language", value: "zh-CN" }),
  ).toEqual({ field: "ui_language", value: "zh-CN" });
  expect(validateSettingsChange({ field: "reset_ui_language" })).toEqual({
    field: "reset_ui_language",
  });
  expect(() =>
    validateSettingsChange({ field: "reset_ui_language", value: "en" }),
  ).toThrow();
});
it("keeps closing actions distinct from on/off choices across languages", () => {
  const doc = { documentElement: { lang: "zh-CN" } } as Document;
  applyUiLanguage(doc, "en");
  expect([
    translate(doc, "关闭"),
    translate(doc, "启用"),
    translate(doc, "停用"),
  ]).toEqual(["Close", "On", "Off"]);
  applyUiLanguage(doc, "zh-CN");
  expect([
    translate(doc, "关闭"),
    translate(doc, "启用"),
    translate(doc, "停用"),
  ]).toEqual(["关闭", "启用", "停用"]);
});
it("all translated templates preserve placeholders; user arguments stay literal", () => {
  for (const [source, english] of Object.entries(englishMessages)) {
    expect(
      [...english.matchAll(/\{\d+\}/g)].map((x) => x[0]).sort(),
      source,
    ).toEqual([...source.matchAll(/\{\d+\}/g)].map((x) => x[0]).sort());
  }
  const doc = { documentElement: { lang: "zh-CN" } } as Document;
  expect(applyUiLanguage(doc, "en")).toBe(true);
  expect(translate(doc, "当前：{0} · {1}", "保存", message("用户设置"))).toBe(
    "Current: 保存 · User settings",
  );
  expect(applyUiLanguage(doc, "zh-CN")).toBe(true);
  expect(translate(doc, "当前：{0} · {1}", "Save", message("用户设置"))).toBe(
    "当前：Save · 用户设置",
  );
});
it("an aborted page cannot apply a late language response", () => {
  const doc = { documentElement: { lang: "zh-CN" } } as Document,
    life = new AbortController();
  applyUiLanguage(doc, "en", life.signal);
  life.abort();
  expect(applyUiLanguage(doc, "zh-CN", life.signal)).toBe(false);
  expect(applyUiLanguage(doc, "zh-CN")).toBe(false);
  expect(doc.documentElement.lang).toBe("en");
});

it("SSH messages translate instructions while retaining user labels and fingerprints", () => {
  const doc = { documentElement: { lang: "zh-CN" } } as Document;
  applyUiLanguage(doc, "en");
  const fingerprint = "SHA256:AbCdEf1234";
  const prompt = translate(
    doc,
    "第 {0}/{1} 跳：{2}，{3}@{4}:{5}。身份 {6}。{7} {8}。{9}",
    1,
    2,
    "保存",
    "用户",
    "host.invalid",
    22,
    "删除",
    "ssh-ed25519",
    fingerprint,
    message("首次指纹，请核对后信任并连接。"),
  );
  expect(prompt).toBe(
    `Hop 1/2: 保存, 用户@host.invalid:22. Identity 删除. ssh-ed25519 ${fingerprint}. First-use fingerprint. Verify it before trusting and connecting.`,
  );
  expect(
    translate(
      doc,
      "{0} · {1} · 私钥{2}",
      "保存",
      "用户",
      message(" · 未保存凭据"),
    ),
  ).toBe("保存 · 用户 · Private key · Credentials not saved");
  applyUiLanguage(doc, "zh-CN");
  expect(
    translate(doc, "当前主机：{0} · 当前身份：{1}", "Save", "Delete"),
  ).toBe("当前主机：Save · 当前身份：Delete");
});
