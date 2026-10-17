import { afterEach, expect, it, vi } from "vitest";
import {
  decodeProfileConfig,
  decodeProfileEditorOffer,
  ProfileEditor,
  profileDisplay,
} from "./workspace-profile-editor";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa",
  ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const config = {
  args: ["", " ; $(literal) 中文🌍 "],
  cwd: "/data",
  env: [["PROFILE_VALUE", "value = 中文"]],
  envRemove: ["OLD_VALUE"],
  appearance: { fontSize: 18, fontFamily: "Pulse Mono", cursorBlink: false },
};
const offer = { ticket, shell: "sh", config };
function fixture() {
  const life = new AbortController();
  const native = {
    read: vi.fn(async (): Promise<unknown> => offer),
    save: vi.fn(async (): Promise<unknown> => null),
    cancel: vi.fn(async () => null),
  };
  return {
    life,
    native,
    editor: new ProfileEditor(nonce, life.signal, native),
  };
}
afterEach(() => vi.useRealTimers());
it("keeps literal empty and Unicode argv, env values, and sparse appearance", () => {
  const decoded = decodeProfileEditorOffer(offer);
  expect(decoded.config.args).toEqual(config.args);
  expect(decoded.config.env).toEqual(config.env);
  expect(Object.isFrozen(decoded.config.args)).toBe(true);
  expect(
    decodeProfileConfig({
      ...config,
      appearance: { fontSize: null, fontFamily: null, cursorBlink: null },
    }).appearance.fontSize,
  ).toBe(null);
});
it.each([
  "TERM_PROGRAM",
  "term_program_version",
  "Pulse_Socket",
  "PULSE_PANE_ID",
  "pulse_future_authority",
  "KEY=VALUE",
  "0KEY",
  "",
])(
  "rejects reserved or malformed key %s in both override and removal",
  (key) => {
    expect(() =>
      decodeProfileConfig({ ...config, env: [[key, "value"]] }),
    ).toThrow();
    expect(() =>
      decodeProfileConfig({ ...config, env: [], envRemove: [key] }),
    ).toThrow();
  },
);
it("rejects raw executables, relative cwd, duplicate keys, NUL, and byte/item limits", () => {
  for (const bad of [
    {
      ...config,
      cwd: null,
      env: [],
      envRemove: [],
      args: ["a".repeat(16380)],
      appearance: { ...config.appearance, fontFamily: "Mono" },
    },
    { ...config, program: "/bin/sh" },
    { ...config, cwd: "~/home" },
    { ...config, args: ["bad\0"] },
    { ...config, args: Array(65).fill("") },
    { ...config, args: ["界".repeat(5500)] },
    { ...config, env: [...config.env, ["profile_value", "duplicate"]] },
    { ...config, envRemove: ["PROFILE_VALUE"] },
    { ...config, appearance: { ...config.appearance, fontSize: 73 } },
    {
      ...config,
      appearance: { ...config.appearance, fontFamily: "url(secret)" },
    },
  ])
    expect(() => decodeProfileConfig(bad)).toThrow();
});
it("uses original nonce and exact config once; save never starts a session", async () => {
  const f = fixture();
  expect(await f.editor.read("sh")).toEqual(offer);
  expect(await f.editor.save(decodeProfileConfig(config))).toBe("saved");
  expect(f.native.save).toHaveBeenCalledWith(nonce, ticket, config);
  expect(await f.editor.save(decodeProfileConfig(config))).toBe("denied");
  expect(f.native.save).toHaveBeenCalledOnce();
  f.editor.close();
});
it("cancelled original read cancels its own late receipt", async () => {
  const f = fixture();
  let finish!: (v: unknown) => void;
  f.native.read.mockImplementation(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const read = f.editor.read("sh");
  f.life.abort();
  expect(await read).toBe(null);
  finish(offer);
  await Promise.resolve();
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  expect(f.native.save).not.toHaveBeenCalled();
});
it("a mismatched shell receipt is cancelled rather than shown or saved", async () => {
  const f = fixture();
  f.native.read.mockResolvedValue({ ...offer, shell: "bash" });
  expect(await f.editor.read("sh")).toBe(null);
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  f.editor.close();
});
it("read timeout rejects late permission and save timeout never retries", async () => {
  vi.useFakeTimers();
  const f = fixture();
  let finish!: (v: unknown) => void;
  f.native.read.mockImplementationOnce(
    () =>
      new Promise((r) => {
        finish = r;
      }),
  );
  const reading = f.editor.read("sh");
  await vi.advanceTimersByTimeAsync(5000);
  expect(await reading).toBe(null);
  finish(offer);
  await Promise.resolve();
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
  await f.editor.read("sh");
  f.native.save.mockImplementation(() => new Promise(() => {}));
  const saving = f.editor.save(decodeProfileConfig(config));
  await vi.advanceTimersByTimeAsync(5000);
  expect(await saving).toBe("uncertain");
  expect(f.native.save).toHaveBeenCalledOnce();
  expect(await f.editor.save(decodeProfileConfig(config))).toBe("denied");
  f.editor.close();
});
it("navigation during save revokes exactly the active ticket", async () => {
  const f = fixture();
  await f.editor.read("sh");
  f.native.save.mockImplementation(() => new Promise(() => {}));
  const saving = f.editor.save(decodeProfileConfig(config));
  f.life.abort();
  expect(await saving).toBe("cancelled");
  expect(f.native.cancel).toHaveBeenCalledWith(nonce, ticket);
});
it("expired read cannot save", async () => {
  vi.useFakeTimers();
  const f = fixture();
  await f.editor.read("sh");
  await vi.advanceTimersByTimeAsync(55000);
  expect(await f.editor.save(decodeProfileConfig(config))).toBe("denied");
  expect(f.native.save).not.toHaveBeenCalled();
  f.editor.close();
});
it("drawing overrides preserve clipboard preferences and inherit unspecified values", () => {
  const base = {
    fontSize: 14,
    fontFamily: "Pulse Mono",
    cursorBlink: true,
    copyOnSelect: false,
    rightClickPaste: false,
  };
  expect(
    profileDisplay(base, {
      fontSize: 18,
      fontFamily: null,
      cursorBlink: false,
    }),
  ).toEqual({ ...base, fontSize: 18, cursorBlink: false });
  expect(
    profileDisplay(undefined, {
      fontSize: null,
      fontFamily: null,
      cursorBlink: null,
    }),
  ).toBeUndefined();
  expect(() =>
    profileDisplay(base, {
      fontSize: 18,
      fontFamily: "x; url(bad)",
      cursorBlink: false,
    }),
  ).toThrow();
});

it("merges sparse Profile Meta, Backspace and Delete overrides without granting input", () => {
  const appearance = {
    fontSize: null,
    fontFamily: null,
    cursorBlink: null,
    macOptionIsMeta: false,
    backspaceSend: "bs",
    deleteSend: "del",
  };
  const decoded = decodeProfileConfig({ ...config, appearance });
  const base = {
    fontSize: 14,
    fontFamily: "monospace",
    cursorBlink: false,
    macOptionIsMeta: true,
    backspaceSend: "del" as const,
    deleteSend: "csi" as const,
  };
  expect(profileDisplay(base, decoded.appearance)).toMatchObject({
    macOptionIsMeta: false,
    backspaceSend: "bs",
    deleteSend: "del",
  });
  expect(
    profileDisplay(base, {
      ...decoded.appearance,
      macOptionIsMeta: null,
      backspaceSend: null,
      deleteSend: null,
    }),
  ).toEqual(base);
  for (const change of [
    { deleteSend: "bs" },
    { backspaceSend: "csi" },
    { macOptionIsMeta: 1 },
    { execute: true },
  ])
    expect(() =>
      decodeProfileConfig({
        ...config,
        appearance: { ...appearance, ...change },
      }),
    ).toThrow();
});

it("keeps palette overrides sparse and out of xterm display preferences", () => {
  for (const colorPreset of ["classic", "midnight", "paper", null]) {
    const decoded = decodeProfileConfig({
      ...config,
      appearance: { ...config.appearance, colorPreset },
    });
    expect(decoded.appearance.colorPreset).toBe(colorPreset);
    expect(profileDisplay(undefined, decoded.appearance)).not.toHaveProperty(
      "colorPreset",
    );
  }
  for (const colorPreset of [undefined, "system", "#ffffff", 1, {}]) {
    expect(() =>
      decodeProfileConfig({
        ...config,
        appearance: { ...config.appearance, colorPreset },
      }),
    ).toThrow();
  }
});
