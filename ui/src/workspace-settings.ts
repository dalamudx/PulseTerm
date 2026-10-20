import { invoke } from "@tauri-apps/api/core";
import { isUiAccent, type UiAccent } from "./workspace-accent";
import { isUiAppearance, type UiAppearance } from "./workspace-appearance";
import { isUiLanguage, type UiLanguage } from "./workspace-language";
import {
  decodeTerminalPresets,
  isTerminalPreset,
  type TerminalPreset,
  type TerminalPresetPaint,
} from "./workspace-terminal-preset";
import {
  isTerminalTheme,
  type TerminalTheme,
} from "./workspace-terminal-theme";
export type DisplaySetting<T> = Readonly<{
  value: T;
  source: "default" | "user";
}>;
export interface WorkspaceSettings {
  readonly version:
    | 1
    | 2
    | 3
    | 4
    | 5
    | 6
    | 7
    | 8
    | 9
    | 10
    | 11
    | 12
    | 13
    | 14
    | 15;
  readonly terminalPresets?: readonly TerminalPresetPaint[];
  readonly token: string;
  readonly values: Readonly<{
    font_size: DisplaySetting<number>;
    history_limit?: DisplaySetting<number>;
    ui_appearance?: DisplaySetting<UiAppearance>;
    ui_language?: DisplaySetting<UiLanguage>;
    ui_accent?: DisplaySetting<UiAccent>;
    terminal_theme?: DisplaySetting<TerminalTheme>;
    terminal_preset?: DisplaySetting<TerminalPreset>;
    line_height_percent?: DisplaySetting<number>;
    letter_spacing?: DisplaySetting<number>;
    font_family: DisplaySetting<string>;
    font_fallback?: DisplaySetting<string>;
    font_ligatures?: DisplaySetting<boolean>;
    cursor_blink: DisplaySetting<boolean>;
    scroll_on_input?: DisplaySetting<boolean>;
    scroll_on_output?: DisplaySetting<boolean>;
    mac_option_as_meta?: DisplaySetting<boolean>;
    backspace_send?: DisplaySetting<"del" | "bs">;
    delete_send?: DisplaySetting<"csi" | "del">;
    term_type?: DisplaySetting<"xterm-256color" | "xterm" | "vt100">;
    copy_on_select: DisplaySetting<boolean>;
    right_click_paste: DisplaySetting<boolean>;
    confirm_close?: DisplaySetting<boolean>;
  }>;
}
export type SettingsChange = Readonly<
  | { field: "history_limit"; value: number }
  | { field: "reset_history_limit" }
  | { field: "ui_appearance"; value: UiAppearance }
  | { field: "ui_language"; value: UiLanguage }
  | { field: "ui_accent"; value: UiAccent }
  | { field: "terminal_theme"; value: TerminalTheme }
  | { field: "terminal_preset"; value: TerminalPreset }
  | { field: "font_size"; value: number }
  | { field: "line_height_percent"; value: number }
  | { field: "letter_spacing"; value: number }
  | { field: "font_family"; value: string }
  | { field: "font_fallback"; value: string }
  | { field: "font_ligatures"; value: boolean }
  | { field: "cursor_blink"; value: boolean }
  | { field: "backspace_send"; value: "del" | "bs" }
  | { field: "delete_send"; value: "csi" | "del" }
  | { field: "term_type"; value: "xterm-256color" | "xterm" | "vt100" }
  | { field: "scroll_on_input"; value: boolean }
  | { field: "scroll_on_output"; value: boolean }
  | { field: "mac_option_as_meta"; value: boolean }
  | { field: "copy_on_select"; value: boolean }
  | { field: "right_click_paste"; value: boolean }
  | { field: "confirm_close"; value: boolean }
  | {
      field:
        | "reset_ui_appearance"
        | "reset_ui_language"
        | "reset_ui_accent"
        | "reset_terminal_theme"
        | "reset_terminal_preset"
        | "reset_font_size"
        | "reset_line_height_percent"
        | "reset_letter_spacing"
        | "reset_font_family"
        | "reset_font_fallback"
        | "reset_font_ligatures"
        | "reset_cursor_blink"
        | "reset_scroll_on_input"
        | "reset_scroll_on_output"
        | "reset_mac_option_as_meta"
        | "reset_backspace_send"
        | "reset_delete_send"
        | "reset_term_type"
        | "reset_copy_on_select"
        | "reset_right_click_paste"
        | "reset_confirm_close";
    }
>;
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
function bad(): never {
  throw Error("settings.invalid");
}
function record(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (!value || typeof value !== "object" || Array.isArray(value)) return bad();
  const r = value as Record<string, unknown>;
  if (
    Object.keys(r).length !== keys.length ||
    keys.some((key) => !Object.hasOwn(r, key))
  )
    return bad();
  return r;
}
function font(value: unknown): value is string {
  return (
    typeof value === "string" &&
    Array.from(value).length <= 128 &&
    /^[\p{L}\p{N}_-](?:[\p{L}\p{N} _-]*[\p{L}\p{N}_-])?$/u.test(value)
  );
}
function lineHeight(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 100 &&
    value <= 300
  );
}
function spacing(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 0 &&
    value <= 8
  );
}
function size(value: unknown): value is number {
  return (
    typeof value === "number" &&
    Number.isInteger(value) &&
    value >= 6 &&
    value <= 72
  );
}
function setting<T>(
  value: unknown,
  validate: (value: unknown) => value is T,
): DisplaySetting<T> {
  const r = record(value, ["value", "source"]);
  if (!validate(r.value) || (r.source !== "default" && r.source !== "user"))
    return bad();
  return Object.freeze({ value: r.value, source: r.source });
}
export function decodeWorkspaceSettings(input: unknown): WorkspaceSettings {
  const version =
    input && typeof input === "object"
      ? (input as Record<string, unknown>).version
      : undefined;
  const r = record(input, [
    "version",
    "token",
    "values",
    ...(version === 5 ||
    version === 6 ||
    version === 7 ||
    version === 8 ||
    version === 9 ||
    version === 10 ||
    version === 11 ||
    version === 12 ||
    version === 13 ||
    version === 14 ||
    version === 15
      ? ["terminal_presets"]
      : []),
  ]);
  if (
    (r.version !== 1 &&
      r.version !== 2 &&
      r.version !== 3 &&
      r.version !== 4 &&
      r.version !== 5 &&
      r.version !== 6 &&
      r.version !== 7 &&
      r.version !== 8 &&
      r.version !== 9 &&
      r.version !== 10 &&
      r.version !== 11 &&
      r.version !== 12 &&
      r.version !== 13 &&
      r.version !== 14 &&
      r.version !== 15) ||
    typeof r.token !== "string" ||
    !uuid.test(r.token)
  )
    return bad();
  const v = record(r.values, [
    "font_size",
    "font_family",
    "cursor_blink",
    "copy_on_select",
    "right_click_paste",
    ...(r.values &&
    typeof r.values === "object" &&
    Object.hasOwn(r.values, "confirm_close")
      ? ["confirm_close"]
      : []),
    ...(r.version !== 1 ? ["line_height_percent", "letter_spacing"] : []),
    ...(r.version === 3 || r.version >= 4
      ? ["font_fallback", "font_ligatures"]
      : []),
    ...(r.version >= 4 ? ["ui_appearance"] : []),
    ...(r.version >= 15 ? ["ui_language"] : []),
    ...(r.version >= 12 ? ["ui_accent"] : []),
    ...(r.version >= 14 ? ["terminal_theme"] : []),
    ...(r.version >= 5 ? ["terminal_preset"] : []),
    ...(r.version >= 6 ? ["history_limit"] : []),
    ...(r.version >= 7 ? ["scroll_on_input"] : []),
    ...(r.version >= 8 ? ["scroll_on_output"] : []),
    ...(r.version >= 9 ? ["mac_option_as_meta"] : []),
    ...(r.version >= 10 ? ["backspace_send"] : []),
    ...(r.version >= 11 ? ["delete_send", "term_type"] : []),
  ]);
  const uiAppearance =
    r.version >= 4 ? setting(v.ui_appearance, isUiAppearance) : undefined;
  if (
    uiAppearance &&
    uiAppearance.source !== "user" &&
    uiAppearance.source !== "default"
  )
    return bad();
  return Object.freeze({
    version: r.version,
    token: r.token,
    ...(r.version >= 5
      ? {
          terminalPresets: decodeTerminalPresets(
            r.terminal_presets,
            r.version >= 13 ? "builtin" : "legacy",
          ),
        }
      : {}),
    values: Object.freeze({
      ...(r.version >= 15
        ? { ui_language: setting(v.ui_language, isUiLanguage) }
        : {}),
      ...(uiAppearance ? { ui_appearance: uiAppearance } : {}),
      ...(r.version >= 12
        ? { ui_accent: setting(v.ui_accent, isUiAccent) }
        : {}),
      ...(r.version >= 14
        ? {
            terminal_theme: (() => {
              const theme = setting(v.terminal_theme, isTerminalTheme);
              return Object.freeze({
                ...theme,
                value: Object.freeze({ ...theme.value }),
              });
            })(),
          }
        : {}),
      ...(r.version >= 5
        ? {
            terminal_preset: setting(
              v.terminal_preset,
              (value): value is TerminalPreset =>
                isTerminalPreset(value) &&
                (r.version === 13 ||
                  r.version === 14 ||
                  ["classic", "midnight", "paper"].includes(value)),
            ),
          }
        : {}),
      font_size: setting(v.font_size, size),
      ...(r.version >= 6
        ? {
            history_limit: setting(
              v.history_limit,
              (x): x is number =>
                typeof x === "number" &&
                Number.isInteger(x) &&
                x >= 1000 &&
                x <= 100000,
            ),
          }
        : {}),
      ...(r.version !== 1
        ? {
            line_height_percent: setting(v.line_height_percent, lineHeight),
            letter_spacing: setting(v.letter_spacing, spacing),
          }
        : {}),
      font_family: setting(v.font_family, font),
      ...(r.version === 3 || r.version >= 4
        ? {
            font_fallback: setting(v.font_fallback, font),
            font_ligatures: setting(
              v.font_ligatures,
              (x): x is boolean => typeof x === "boolean",
            ),
          }
        : {}),
      ...(r.version >= 7
        ? {
            scroll_on_input: setting(
              v.scroll_on_input,
              (x): x is boolean => typeof x === "boolean",
            ),
          }
        : {}),
      ...(r.version >= 8
        ? {
            scroll_on_output: setting(
              v.scroll_on_output,
              (x): x is boolean => typeof x === "boolean",
            ),
          }
        : {}),
      ...(r.version >= 9
        ? {
            mac_option_as_meta: setting(
              v.mac_option_as_meta,
              (x): x is boolean => typeof x === "boolean",
            ),
          }
        : {}),
      ...(r.version >= 10
        ? {
            backspace_send: setting(
              v.backspace_send,
              (x): x is "del" | "bs" => x === "del" || x === "bs",
            ),
          }
        : {}),
      ...(r.version >= 11
        ? {
            delete_send: setting(
              v.delete_send,
              (x): x is "csi" | "del" => x === "csi" || x === "del",
            ),
            term_type: setting(
              v.term_type,
              (x): x is "xterm-256color" | "xterm" | "vt100" =>
                x === "xterm-256color" || x === "xterm" || x === "vt100",
            ),
          }
        : {}),
      cursor_blink: setting(
        v.cursor_blink,
        (x): x is boolean => typeof x === "boolean",
      ),
      copy_on_select: setting(
        v.copy_on_select,
        (x): x is boolean => typeof x === "boolean",
      ),
      right_click_paste: setting(
        v.right_click_paste,
        (x): x is boolean => typeof x === "boolean",
      ),
      ...(Object.hasOwn(v, "confirm_close")
        ? {
            confirm_close: setting(
              v.confirm_close,
              (x): x is boolean => typeof x === "boolean",
            ),
          }
        : {}),
    }),
  });
}
export function validateSettingsChange(value: unknown): SettingsChange {
  if (!value || typeof value !== "object") return bad();
  const field = (value as Record<string, unknown>).field;
  if (
    field === "reset_history_limit" ||
    field === "reset_terminal_theme" ||
    field === "reset_terminal_preset" ||
    field === "reset_ui_appearance" ||
    field === "reset_ui_language" ||
    field === "reset_ui_accent" ||
    field === "reset_font_size" ||
    field === "reset_line_height_percent" ||
    field === "reset_letter_spacing" ||
    field === "reset_font_family" ||
    field === "reset_font_fallback" ||
    field === "reset_font_ligatures" ||
    field === "reset_cursor_blink" ||
    field === "reset_scroll_on_input" ||
    field === "reset_scroll_on_output" ||
    field === "reset_mac_option_as_meta" ||
    field === "reset_backspace_send" ||
    field === "reset_delete_send" ||
    field === "reset_term_type" ||
    field === "reset_copy_on_select" ||
    field === "reset_right_click_paste" ||
    field === "reset_confirm_close"
  ) {
    record(value, ["field"]);
    return Object.freeze({ field });
  }
  const r = record(value, ["field", "value"]);
  if (field === "ui_language" && isUiLanguage(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "terminal_theme" && isTerminalTheme(r.value))
    return Object.freeze({ field, value: Object.freeze({ ...r.value }) });
  if (
    field === "history_limit" &&
    typeof r.value === "number" &&
    Number.isInteger(r.value) &&
    r.value >= 1000 &&
    r.value <= 100000
  )
    return Object.freeze({ field, value: r.value });
  if (field === "terminal_preset" && isTerminalPreset(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "ui_accent" && isUiAccent(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "ui_appearance" && isUiAppearance(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "font_size" && size(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "delete_send" && (r.value === "csi" || r.value === "del"))
    return Object.freeze({ field, value: r.value });
  if (
    field === "term_type" &&
    (r.value === "xterm-256color" || r.value === "xterm" || r.value === "vt100")
  )
    return Object.freeze({ field, value: r.value });
  if (field === "backspace_send" && (r.value === "del" || r.value === "bs"))
    return Object.freeze({ field, value: r.value });
  if (field === "line_height_percent" && lineHeight(r.value))
    return Object.freeze({ field, value: r.value });
  if (field === "letter_spacing" && spacing(r.value))
    return Object.freeze({ field, value: r.value });
  if ((field === "font_family" || field === "font_fallback") && font(r.value))
    return Object.freeze({ field, value: r.value });
  if (
    (field === "cursor_blink" ||
      field === "scroll_on_input" ||
      field === "scroll_on_output" ||
      field === "mac_option_as_meta" ||
      field === "font_ligatures" ||
      field === "copy_on_select" ||
      field === "right_click_paste" ||
      field === "confirm_close") &&
    typeof r.value === "boolean"
  )
    return Object.freeze({ field, value: r.value });
  return bad();
}
export type SettingsRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: SettingsRpc,
): Promise<WorkspaceSettings> {
  if (signal.aborted) throw Error("settings.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort!: () => void;
  const ended = new Promise<never>((_, reject) => {
    abort = () => reject(Error("settings.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("settings.timeout")), 5000);
  });
  try {
    const result = await Promise.race([rpc(command, args), ended]);
    if (signal.aborted) throw Error("settings.cancelled");
    return decodeWorkspaceSettings(result);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export function readWorkspaceSettings(
  nonce: string,
  signal: AbortSignal,
  rpc: SettingsRpc = invoke,
): Promise<WorkspaceSettings> {
  if (!uuid.test(nonce)) throw Error("settings.invalid");
  return request("workspace_settings_read", { nonce }, signal, rpc);
}
export function saveWorkspaceSettings(
  nonce: string,
  token: string,
  change: SettingsChange,
  signal: AbortSignal,
  rpc: SettingsRpc = invoke,
): Promise<WorkspaceSettings> {
  if (!uuid.test(nonce) || !uuid.test(token)) throw Error("settings.invalid");
  return request(
    "workspace_settings_save",
    { nonce, token, change: validateSettingsChange(change) },
    signal,
    rpc,
  );
}
