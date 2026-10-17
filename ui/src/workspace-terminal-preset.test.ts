import { expect, it, vi } from "vitest";
import {
  decodeWorkspaceSettings,
  validateSettingsChange,
  type WorkspaceSettings,
} from "./workspace-settings";
import { WorkspaceSettingsEditor } from "./workspace-settings-editor";
import {
  decodeTerminalPresets,
  terminalPresetField,
  terminalPresetIds,
} from "./workspace-terminal-preset";
import { terminalThemeField } from "./workspace-terminal-theme";

class Node extends EventTarget {
  children: (Node | string)[] = [];
  textContent = "";
  className = "";
  value = "";
  type = "";
  disabled = false;
  hidden = false;
  open = false;
  style: Record<string, string> = {};
  constructor(
    readonly tag: string,
    readonly ownerDocument: Document,
  ) {
    super();
  }
  append(...children: (Node | string)[]) {
    this.children.push(...children);
  }
  replaceChildren(...children: Node[]) {
    this.children = children;
  }
  setAttribute() {}
  removeAttribute() {}
  showModal() {
    this.open = true;
  }
  close() {
    this.open = false;
    this.dispatchEvent(new Event("close"));
  }
  querySelectorAll(query: string): Node[] {
    return this.children.flatMap((child) =>
      typeof child === "string"
        ? []
        : [
            ...(query.split(",").includes(child.tag) ? [child] : []),
            ...child.querySelectorAll(query),
          ],
    );
  }
}
function fixture() {
  const document = {
    createElement: (tag: string) =>
      new Node(tag, document as unknown as Document),
  };
  return document as unknown as Document;
}
function child(root: unknown, tag: string): Node {
  const result = (root as Node).querySelectorAll(tag)[0];
  if (!result) throw Error("missing " + tag);
  return result;
}
const paints = () =>
  ["classic", "midnight", "paper"].map((id, n) => ({
    id,
    colors: {
      foreground: [230, 230, 230],
      background: [n * 10, 0, 0],
      cursor: [230, 230, 230],
      ansi: Array.from({ length: 16 }, () => [120, 120, 120]),
    },
  }));
const projection = () => ({
  version: 5,
  token: "11111111-1111-4111-8111-111111111111",
  terminal_presets: paints(),
  values: {
    font_size: { value: 14, source: "default" },
    font_family: { value: "monospace", source: "default" },
    font_fallback: { value: "monospace", source: "default" },
    font_ligatures: { value: false, source: "default" },
    line_height_percent: { value: 100, source: "default" },
    letter_spacing: { value: 0, source: "default" },
    cursor_blink: { value: false, source: "default" },
    copy_on_select: { value: false, source: "default" },
    right_click_paste: { value: false, source: "default" },
    ui_appearance: { value: "light", source: "default" },
    terminal_preset: { value: "classic", source: "default" },
  },
});
it("decodes fixed native palettes and rejects unknown or malformed substitutions", () => {
  const input = projection(),
    decoded = decodeWorkspaceSettings(input);
  expect(decoded.values.terminal_preset?.value).toBe("classic");
  expect(decoded.terminalPresets).toHaveLength(3);
  for (const values of [
    [],
    [paints()[0], paints()[0], paints()[2]],
    paints().map((p) => ({ ...p, id: "external" })),
    paints().map((p) => ({
      ...p,
      colors: { ...p.colors, background: [256, 0, 0] },
    })),
  ])
    expect(() => decodeTerminalPresets(values)).toThrow();
  expect(() => decodeWorkspaceSettings({ ...input, version: 4 })).toThrow();
  expect(Object.isFrozen(decoded.terminalPresets?.[0]?.colors.background)).toBe(
    true,
  );
  expect(
    validateSettingsChange({ field: "terminal_preset", value: "paper" }),
  ).toEqual({ field: "terminal_preset", value: "paper" });
  expect(() =>
    validateSettingsChange({ field: "terminal_preset", value: "external" }),
  ).toThrow();
});
it("accepts the complete v13 catalogue and rejects partial or legacy substitutions", () => {
  const base = projection();
  const paints13 = terminalPresetIds.map((id) => ({ ...paints()[0], id }));
  const input = {
    ...base,
    version: 13,
    terminal_presets: paints13,
    values: {
      ...base.values,
      terminal_preset: { value: "dracula", source: "user" },
      ui_accent: { value: "cyan", source: "default" },
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
      scroll_on_output: { value: false, source: "default" },
      mac_option_as_meta: { value: false, source: "default" },
      backspace_send: { value: "del", source: "default" },
      delete_send: { value: "csi", source: "default" },
      term_type: { value: "xterm-256color", source: "default" },
    },
  };
  expect(
    decodeWorkspaceSettings(input).terminalPresets?.map((p) => p.id),
  ).toEqual(terminalPresetIds);
  for (const id of terminalPresetIds)
    expect(
      validateSettingsChange({ field: "terminal_preset", value: id }),
    ).toEqual({ field: "terminal_preset", value: id });
  for (const terminal_presets of [
    paints(),
    paints13.slice(1),
    paints13.map((p, i) => (i === 1 ? paints13[0] : p)),
    paints13.map((p, i) => (i === 1 ? { ...p, id: "url(file)" } : p)),
  ]) {
    expect(() =>
      decodeWorkspaceSettings({ ...input, terminal_presets }),
    ).toThrow();
  }
  expect(() => decodeWorkspaceSettings({ ...input, version: 12 })).toThrow();
  expect(() =>
    decodeWorkspaceSettings({
      ...base,
      values: {
        ...base.values,
        terminal_preset: { value: "dracula", source: "user" },
      },
    }),
  ).toThrow();
  const save = vi.fn();
  const field = terminalPresetField({
    document: fixture(),
    setting: { value: "classic", source: "default" },
    presets: decodeTerminalPresets(paints13, "builtin"),
    signal: new AbortController().signal,
    current: () => true,
    save,
  });
  const select = child(field, "select");
  expect(select.children).toHaveLength(10);
  select.value = "catppuccin_latte";
  select.dispatchEvent(new Event("change"));
  expect(save).not.toHaveBeenCalled();
  field.dispatchEvent(new Event("submit"));
  expect(save).toHaveBeenCalledWith({
    field: "terminal_preset",
    value: "catppuccin_latte",
  });
});
it("previews locally and only submits the selected scalar or reset", () => {
  const save = vi.fn(),
    controller = new AbortController(),
    document = fixture();
  const field = terminalPresetField({
    document,
    setting: { value: "classic", source: "default" },
    presets: decodeTerminalPresets(paints()),
    signal: controller.signal,
    current: () => true,
    save,
  });
  const select = child(field, "select");
  select.value = "paper";
  select.dispatchEvent(new Event("change"));
  expect(child(field, "pre").style.backgroundColor).toBe("rgb(20, 0, 0)");
  expect(save).not.toHaveBeenCalled();
  expect(child(field, "p").textContent).toContain("SSH");
  expect(child(field, "p").textContent).toContain("已有终端保持原配色");
  field.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(save).toHaveBeenLastCalledWith({
    field: "terminal_preset",
    value: "paper",
  });
  const reset = (field as unknown as Node)
    .querySelectorAll("button")
    .find((button) => button.type === "button");
  reset?.dispatchEvent(new Event("click"));
  expect(save).toHaveBeenLastCalledWith({ field: "reset_terminal_preset" });
  controller.abort();
});
it("disposed or stale forms cannot preview or submit", () => {
  const save = vi.fn(),
    controller = new AbortController(),
    document = fixture();
  let current = true;
  const field = terminalPresetField({
    document,
    setting: { value: "classic", source: "default" },
    presets: decodeTerminalPresets(paints()),
    signal: controller.signal,
    current: () => current,
    save,
  });
  current = false;
  const select = child(field, "select");
  select.value = "paper";
  select.dispatchEvent(new Event("change"));
  field.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(child(field, "pre").style.backgroundColor).toBe("rgb(0, 0, 0)");
  controller.abort();
  current = true;
  field.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(save).not.toHaveBeenCalled();
});
it("renders a preview on editor load and after saving a new preset", async () => {
  const document = fixture(),
    fields = document.createElement("section"),
    status = document.createElement("p"),
    dialog = document.createElement("dialog");
  const first = decodeWorkspaceSettings(projection());
  const raw = projection();
  raw.values.terminal_preset = { value: "paper", source: "user" };
  const saved = decodeWorkspaceSettings(raw);
  const save = vi.fn(async () => saved);
  const editor = new WorkspaceSettingsEditor(
    {
      signal: new AbortController().signal,
      dialog: dialog as HTMLDialogElement,
      fields,
      status,
      nonce: () => "11111111-1111-4111-8111-111111111111",
      working: () => {},
      changed: (_: WorkspaceSettings) => true,
    },
    { read: async () => first, save },
  );
  editor.open();
  await vi.waitFor(() =>
    expect(child(fields, "pre").style.backgroundColor).toBe("rgb(0, 0, 0)"),
  );
  const form = (fields as unknown as Node)
    .querySelectorAll("form")
    .find((row) => row.querySelectorAll("pre").length);
  if (!form) throw Error("missing preset form");
  const select = child(form, "select");
  select.value = "paper";
  form.dispatchEvent(new Event("submit", { cancelable: true }));
  await vi.waitFor(() =>
    expect(child(fields, "pre").style.backgroundColor).toBe("rgb(20, 0, 0)"),
  );
  expect(save).toHaveBeenCalledWith(
    expect.any(String),
    first.token,
    { field: "terminal_preset", value: "paper" },
    expect.any(AbortSignal),
  );
  editor.dispose();
});

it("v6 history capacity is required, bounded and never admitted by legacy projections", () => {
  const old = projection();
  const next = {
    ...old,
    version: 6,
    values: { ...old.values, history_limit: { value: 2500, source: "user" } },
  };
  expect(decodeWorkspaceSettings(next).values.history_limit?.value).toBe(2500);
  expect(() => decodeWorkspaceSettings({ ...next, version: 5 })).toThrow();
  expect(() => decodeWorkspaceSettings({ ...old, version: 6 })).toThrow();
  for (const value of [0, 999, 100001, 2500.5, "2500", null, true]) {
    expect(() =>
      decodeWorkspaceSettings({
        ...next,
        values: { ...next.values, history_limit: { value, source: "user" } },
      }),
    ).toThrow();
    expect(() =>
      validateSettingsChange({ field: "history_limit", value }),
    ).toThrow();
  }
  expect(
    validateSettingsChange({ field: "history_limit", value: 1000 }),
  ).toEqual({ field: "history_limit", value: 1000 });
  expect(validateSettingsChange({ field: "reset_history_limit" })).toEqual({
    field: "reset_history_limit",
  });
});

it("v7 requires a boolean input scroll preference and preserves old projections", () => {
  const input = {
    ...projection(),
    version: 7,
    values: {
      ...projection().values,
      history_limit: { value: 2000, source: "default" },
      scroll_on_input: { value: false, source: "user" },
    },
  };
  const decoded = decodeWorkspaceSettings(input);
  expect(decoded.values.scroll_on_input).toEqual({
    value: false,
    source: "user",
  });
  expect(Object.isFrozen(decoded.values.scroll_on_input)).toBe(true);
  const values = Object.fromEntries(
    Object.entries(input.values).filter(([key]) => key !== "scroll_on_input"),
  );
  expect(() => decodeWorkspaceSettings({ ...input, values })).toThrow();
  expect(
    decodeWorkspaceSettings({ ...input, version: 6, values }).values
      .scroll_on_input,
  ).toBeUndefined();
  expect(() => decodeWorkspaceSettings({ ...input, version: 6 })).toThrow();
  for (const value of [0, "false", null])
    expect(() =>
      decodeWorkspaceSettings({
        ...input,
        values: { ...input.values, scroll_on_input: { value, source: "user" } },
      }),
    ).toThrow();
});

it("v8 requires boolean output scrolling while v7 preserves the previous default", () => {
  const old = {
    ...projection(),
    version: 7,
    values: {
      ...projection().values,
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
    },
  };
  const next = {
    ...old,
    version: 8,
    values: {
      ...old.values,
      scroll_on_output: { value: false, source: "default" },
    },
  };
  expect(decodeWorkspaceSettings(old).values.scroll_on_output).toBeUndefined();
  expect(decodeWorkspaceSettings(next).values.scroll_on_output?.value).toBe(
    false,
  );
  expect(() => decodeWorkspaceSettings({ ...next, version: 7 })).toThrow();
  expect(() => decodeWorkspaceSettings({ ...old, version: 8 })).toThrow();
  for (const value of [0, "false", null])
    expect(() =>
      decodeWorkspaceSettings({
        ...next,
        values: { ...next.values, scroll_on_output: { value, source: "user" } },
      }),
    ).toThrow();
  expect(
    validateSettingsChange({ field: "scroll_on_output", value: true }),
  ).toEqual({ field: "scroll_on_output", value: true });
  expect(validateSettingsChange({ field: "reset_scroll_on_output" })).toEqual({
    field: "reset_scroll_on_output",
  });
  expect(() =>
    validateSettingsChange({ field: "scroll_on_output", value: "true" }),
  ).toThrow();
});

it("v9 validates Option Meta and preserves old projection defaults", () => {
  const old = {
    ...projection(),
    version: 8,
    values: {
      ...projection().values,
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
      scroll_on_output: { value: false, source: "default" },
    },
  };
  expect(
    decodeWorkspaceSettings(old).values.mac_option_as_meta,
  ).toBeUndefined();
  for (const value of [false, true]) {
    const next = {
      ...old,
      version: 9,
      values: { ...old.values, mac_option_as_meta: { value, source: "user" } },
    };
    expect(decodeWorkspaceSettings(next).values.mac_option_as_meta?.value).toBe(
      value,
    );
    expect(() => decodeWorkspaceSettings({ ...next, version: 8 })).toThrow();
  }
  expect(() => decodeWorkspaceSettings({ ...old, version: 9 })).toThrow();
  for (const value of [0, "true", null]) {
    expect(() =>
      decodeWorkspaceSettings({
        ...old,
        version: 9,
        values: {
          ...old.values,
          mac_option_as_meta: { value, source: "user" },
        },
      }),
    ).toThrow();
    expect(() =>
      validateSettingsChange({ field: "mac_option_as_meta", value }),
    ).toThrow();
  }
  expect(
    validateSettingsChange({ field: "mac_option_as_meta", value: false }),
  ).toEqual({ field: "mac_option_as_meta", value: false });
  expect(validateSettingsChange({ field: "reset_mac_option_as_meta" })).toEqual(
    { field: "reset_mac_option_as_meta" },
  );
});

it("v10 requires a closed Backspace value while v9 stays compatible", () => {
  const old = {
    ...projection(),
    version: 9,
    values: {
      ...projection().values,
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
      scroll_on_output: { value: false, source: "default" },
      mac_option_as_meta: { value: false, source: "default" },
    },
  };
  expect(decodeWorkspaceSettings(old).values.backspace_send).toBeUndefined();
  for (const value of ["del", "bs"] as const) {
    const next = {
      ...old,
      version: 10,
      values: { ...old.values, backspace_send: { value, source: "user" } },
    };
    expect(decodeWorkspaceSettings(next).values.backspace_send?.value).toBe(
      value,
    );
    expect(() => decodeWorkspaceSettings({ ...next, version: 9 })).toThrow();
    expect(validateSettingsChange({ field: "backspace_send", value })).toEqual({
      field: "backspace_send",
      value,
    });
  }
  expect(() => decodeWorkspaceSettings({ ...old, version: 10 })).toThrow();
  for (const value of ["ctrl-h", "\b", true, 1, null]) {
    expect(() =>
      validateSettingsChange({ field: "backspace_send", value }),
    ).toThrow();
  }
  expect(validateSettingsChange({ field: "reset_backspace_send" })).toEqual({
    field: "reset_backspace_send",
  });
});

it("v11 requires bounded TERM and Delete choices while older snapshots remain valid", () => {
  const old = projection();
  const next = {
    ...old,
    version: 11,
    values: {
      ...old.values,
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
      scroll_on_output: { value: false, source: "default" },
      mac_option_as_meta: { value: false, source: "default" },
      backspace_send: { value: "del", source: "default" },
      delete_send: { value: "csi", source: "default" },
      term_type: { value: "vt100", source: "user" },
    },
  };
  expect(decodeWorkspaceSettings(next).values.term_type?.value).toBe("vt100");
  expect(decodeWorkspaceSettings(next).values.delete_send?.value).toBe("csi");
  expect(decodeWorkspaceSettings(old).values.term_type).toBeUndefined();
  const { term_type: _term, ...missing } = next.values;
  expect(() => decodeWorkspaceSettings({ ...next, values: missing })).toThrow();
  expect(() => decodeWorkspaceSettings({ ...next, version: 10 })).toThrow();
  for (const value of ["vt100", "xterm", "xterm-256color"])
    expect(validateSettingsChange({ field: "term_type", value })).toEqual({
      field: "term_type",
      value,
    });
  for (const value of ["csi", "del"])
    expect(validateSettingsChange({ field: "delete_send", value })).toEqual({
      field: "delete_send",
      value,
    });
  for (const change of [
    { field: "term_type", value: "custom" },
    { field: "term_type", value: "xterm\n" },
    { field: "delete_send", value: "bs" },
  ])
    expect(() => validateSettingsChange(change)).toThrow();
  expect(validateSettingsChange({ field: "reset_delete_send" })).toEqual({
    field: "reset_delete_send",
  });
});

it("v14 validates an atomic system palette policy and freezes its snapshot", () => {
  const base = projection();
  const input = {
    ...base,
    version: 14,
    terminal_presets: terminalPresetIds.map((id) => ({ ...paints()[0], id })),
    values: {
      ...base.values,
      ui_accent: { value: "cyan", source: "default" },
      history_limit: { value: 100000, source: "default" },
      scroll_on_input: { value: true, source: "default" },
      scroll_on_output: { value: false, source: "default" },
      mac_option_as_meta: { value: false, source: "default" },
      backspace_send: { value: "del", source: "default" },
      delete_send: { value: "csi", source: "default" },
      term_type: { value: "xterm-256color", source: "default" },
      terminal_theme: {
        value: { mode: "system", light: "catppuccin_latte", dark: "dracula" },
        source: "user",
      },
    },
  };
  const result = decodeWorkspaceSettings(input);
  input.values.terminal_theme.value.dark = "midnight";
  expect(result.values.terminal_theme?.value.dark).toBe("dracula");
  expect(Object.isFrozen(result.values.terminal_theme?.value)).toBe(true);
  for (const value of [
    { mode: "system", light: "paper" },
    { mode: "auto", light: "paper", dark: "midnight" },
    { mode: "system", light: "remote", dark: "midnight" },
    { mode: "system", light: "paper", dark: "midnight", css: "red" },
  ]) {
    expect(() =>
      decodeWorkspaceSettings({
        ...input,
        values: { ...input.values, terminal_theme: { value, source: "user" } },
      }),
    ).toThrow();
    expect(() =>
      validateSettingsChange({ field: "terminal_theme", value }),
    ).toThrow();
  }
  expect(
    validateSettingsChange({
      field: "terminal_theme",
      value: result.values.terminal_theme?.value,
    }),
  ).toEqual({
    field: "terminal_theme",
    value: { mode: "system", light: "catppuccin_latte", dark: "dracula" },
  });
  expect(validateSettingsChange({ field: "reset_terminal_theme" })).toEqual({
    field: "reset_terminal_theme",
  });
});
it("system-theme drafts preview independently and save one current policy", () => {
  const save = vi.fn(),
    controller = new AbortController();
  let current = true;
  const field = terminalThemeField({
    document: fixture(),
    setting: {
      value: { mode: "fixed", light: "paper", dark: "midnight" },
      source: "default",
    },
    presets: decodeTerminalPresets(paints()),
    signal: controller.signal,
    current: () => current,
    save,
  });
  const root = field as unknown as Node;
  const [mode, light, dark] = root.querySelectorAll("select");
  if (!mode || !light || !dark) throw Error("missing theme choices");
  expect(light.disabled).toBe(true);
  mode.value = "system";
  mode.dispatchEvent(new Event("change"));
  expect(light.disabled).toBe(false);
  light.value = "classic";
  light.dispatchEvent(new Event("change"));
  expect(root.querySelectorAll("pre")[0]?.style.backgroundColor).toBe(
    "rgb(0, 0, 0)",
  );
  expect(save).not.toHaveBeenCalled();
  field.dispatchEvent(new Event("submit", { cancelable: true }));
  expect(save).toHaveBeenLastCalledWith({
    field: "terminal_theme",
    value: { mode: "system", light: "classic", dark: "midnight" },
  });
  const reset = root.querySelectorAll("button")[1];
  reset?.dispatchEvent(new Event("click"));
  expect(save).toHaveBeenLastCalledWith({ field: "reset_terminal_theme" });
  current = false;
  field.dispatchEvent(new Event("submit", { cancelable: true }));
  controller.abort();
  reset?.dispatchEvent(new Event("click"));
  expect(save).toHaveBeenCalledTimes(2);
});
