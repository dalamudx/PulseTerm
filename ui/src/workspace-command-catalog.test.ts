import { expect, it, vi } from "vitest";
import {
  captureWorkspaceCommands,
  isCommandPaletteShortcut,
  type WorkspaceCommand,
  type WorkspaceCommandId,
  workspaceCommandIds,
} from "./workspace-command-catalog";

it("opening keeps the captured callback/label even if the caller rebuilds the registry", () => {
  const f = fixture(),
    hijack = vi.fn(),
    commands = [f.command];
  const capture = captureWorkspaceCommands(f.context, commands);
  Object.assign(f.command, { run: hijack, title: "replacement" });
  commands.length = 0;
  expect(capture?.list("")).toEqual([
    { id: "close-session", title: "关闭会话" },
  ]);
  expect(capture?.execute("close-session")).toBe(true);
  expect(f.run).toHaveBeenCalledOnce();
  expect(hijack).not.toHaveBeenCalled();
});
function fixture() {
  const life = new AbortController();
  let nonce: string | undefined = "page-one",
    busy = false,
    enabled = true,
    target = "session-one";
  const run = vi.fn();
  const command: WorkspaceCommand = {
    id: "close-session",
    title: "关闭会话",
    keywords: "close terminal",
    available: () => enabled,
    target: () => target,
    run,
  };
  const context = {
    signal: life.signal,
    nonce: () => nonce,
    available: () => !busy,
  };
  const capture = () => captureWorkspaceCommands(context, [command]);
  return {
    life,
    command,
    context,
    capture,
    run,
    setNonce: (v: string | undefined) => {
      nonce = v;
    },
    setBusy: (v: boolean) => {
      busy = v;
    },
    setEnabled: (v: boolean) => {
      enabled = v;
    },
    setTarget: (v: string) => {
      target = v;
    },
  };
}
it("lists bounded literal metadata with Chinese/English search, not native permission", () => {
  const f = fixture(),
    c = f.capture();
  expect(c?.list("关闭")).toEqual([{ id: "close-session", title: "关闭会话" }]);
  expect(c?.list(" CLOSE ")).toHaveLength(1);
  expect(c?.list("[.*]")).toEqual([]);
  expect(c?.list("x".repeat(129))).toEqual([]);
  expect(f.run).not.toHaveBeenCalled();
});
it("rejects missing page, page exit and a different current nonce", () => {
  const f = fixture(),
    c = f.capture();
  f.setNonce("page-two");
  expect(c?.execute("close-session")).toBe(false);
  expect(c?.list("")).toEqual([]);
  f.setNonce(undefined);
  expect(f.capture()).toBeUndefined();
  f.setNonce("page-three");
  const next = f.capture();
  f.life.abort();
  expect(next?.execute("close-session")).toBe(false);
  expect(f.capture()).toBeUndefined();
});
it("rechecks busy and action availability after opening", () => {
  const f = fixture(),
    c = f.capture();
  f.setBusy(true);
  expect(c?.execute("close-session")).toBe(false);
  expect(f.capture()).toBeUndefined();
  f.setBusy(false);
  f.setEnabled(false);
  expect(c?.list("")).toEqual([]);
  expect(c?.execute("close-session")).toBe(false);
  expect(f.run).not.toHaveBeenCalled();
});
it("a target change cannot close the newly selected session using the old display choice", () => {
  const f = fixture(),
    c = f.capture();
  f.setTarget("session-two");
  expect(c?.execute("close-session")).toBe(false);
  expect(c?.list("")).toEqual([]);
  expect(f.capture()?.execute("close-session")).toBe(true);
});
it("rejects unknown execution IDs and closing a capture consumes it", () => {
  const f = fixture(),
    c = f.capture();
  expect(c?.execute("arbitrary-shell" as WorkspaceCommandId)).toBe(false);
  c?.close();
  expect(c?.execute("close-session")).toBe(false);
  expect(f.run).not.toHaveBeenCalled();
});
it("consumes before callback reentrancy and never repeats an action", () => {
  const f = fixture(),
    c = f.capture();
  f.run.mockImplementation(() =>
    expect(c?.execute("close-session")).toBe(false),
  );
  expect(c?.execute("close-session")).toBe(true);
  expect(c?.execute("close-session")).toBe(false);
  expect(c?.list("")).toEqual([]);
  expect(f.run).toHaveBeenCalledOnce();
});
it("a callback failure is not automatically retried", () => {
  const f = fixture(),
    c = f.capture();
  f.run.mockImplementation(() => {
    throw new Error("uncertain");
  });
  expect(() => c?.execute("close-session")).toThrow("uncertain");
  expect(c?.execute("close-session")).toBe(false);
  expect(f.run).toHaveBeenCalledOnce();
});
it("bounds catalog names, count, and duplicate IDs", () => {
  const f = fixture();
  expect(() =>
    captureWorkspaceCommands(f.context, [f.command, f.command]),
  ).toThrow("workspace.commands_invalid");
  expect(() =>
    captureWorkspaceCommands(f.context, [
      { ...f.command, title: "x".repeat(129) },
    ]),
  ).toThrow("workspace.command_text_limit");
  expect(() =>
    captureWorkspaceCommands(
      f.context,
      Array.from({ length: workspaceCommandIds.length + 1 }, () => f.command),
    ),
  ).toThrow("workspace.commands_limit");
});
it("shortcut excludes IME composition, repeat, alt, and previously consumed key events", () => {
  const base = {
    key: "P",
    ctrlKey: true,
    metaKey: false,
    shiftKey: true,
    altKey: false,
    repeat: false,
    isComposing: false,
    defaultPrevented: false,
  };
  expect(isCommandPaletteShortcut(base)).toBe(true);
  expect(
    isCommandPaletteShortcut({ ...base, ctrlKey: false, metaKey: true }),
  ).toBe(true);
  for (const patch of [
    { isComposing: true },
    { repeat: true },
    { altKey: true },
    { defaultPrevented: true },
    { shiftKey: false },
    { key: "r" },
    { ctrlKey: false },
  ])
    expect(isCommandPaletteShortcut({ ...base, ...patch })).toBe(false);
});

it("discovers new actions in both languages without invoking them", () => {
  const f = fixture(),
    run = vi.fn();
  const commands: WorkspaceCommand[] = [
    { ...f.command, id: "keybindings", title: "快捷键设置", run },
    { ...f.command, id: "ssh-config", title: "导入 SSH 配置", run },
  ];
  const capture = captureWorkspaceCommands(f.context, commands);
  expect(capture?.list("keyboard").map((value) => value.id)).toEqual([
    "keybindings",
  ]);
  expect(capture?.list("导入").map((value) => value.id)).toEqual([
    "ssh-config",
  ]);
  expect(capture?.list("import").map((value) => value.id)).toEqual([
    "ssh-config",
  ]);
  expect(run).not.toHaveBeenCalled();
  expect(capture?.execute("keybindings")).toBe(true);
  expect(capture?.execute("keybindings")).toBe(false);
  expect(run).toHaveBeenCalledOnce();
});
it.each(["split-row", "split-column"] as const)(
  "%s rejects changed pane selections",
  (id) => {
    const f = fixture();
    let second = "peer-one";
    const capture = captureWorkspaceCommands(f.context, [
      { ...f.command, id, target: () => JSON.stringify(["first", second]) },
    ]);
    second = "peer-two";
    expect(capture?.execute(id)).toBe(false);
    expect(f.run).not.toHaveBeenCalled();
  },
);
