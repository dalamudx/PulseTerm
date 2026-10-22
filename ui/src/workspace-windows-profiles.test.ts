import { expect, it, vi } from "vitest";
import { decodeDefaultProfile } from "./workspace-profile-default";
import { decodeProfileEditorOffer } from "./workspace-profile-editor";
import {
  decodeDeletedProfiles,
  decodeLifecycleOffer,
} from "./workspace-profile-lifecycle";
import {
  decodeWorkspaceProfiles,
  type ProfileKind,
} from "./workspace-profiles";
import { openWorkspaceProfile } from "./workspace-sessions";

const nonce = "aaaaaaaa-aaaa-4aaa-8aaa-aaaaaaaaaaaa";
const ticket = "bbbbbbbb-bbbb-4bbb-8bbb-bbbbbbbbbbbb";
const config = {
  args: [],
  cwd: null,
  env: [],
  envRemove: [],
  appearance: { fontSize: null, fontFamily: null, cursorBlink: null },
};

it.each(["pwsh", "powershell", "cmd", "wsl"] as const)(
  "keeps %s usable across discovery, default, editing, lifecycle and explicit startup",
  async (kind) => {
    expect(decodeWorkspaceProfiles([{ kind, hidden: false }])[0]?.kind).toBe(
      kind,
    );
    expect(
      decodeDefaultProfile({
        ticket,
        selected: kind,
        available: true,
        offered: [kind],
      }).selected,
    ).toBe(kind);
    expect(
      decodeProfileEditorOffer({ ticket, shell: kind, config }).shell,
    ).toBe(kind);
    expect(decodeDeletedProfiles([kind])).toEqual([kind]);
    expect(
      decodeLifecycleOffer({
        ticket,
        shell: kind,
        action: "restore",
        isDefault: false,
      }).shell,
    ).toBe(kind);
    const send = vi.fn(async () => "1");
    expect(await openWorkspaceProfile(nonce, kind, send)).toBe("1");
    expect(send).toHaveBeenCalledExactlyOnceWith(nonce, kind);
  },
);

it.each([
  "cmd.exe",
  "powershell.exe",
  "wsl --distribution external",
  "C:\\Windows\\System32\\cmd.exe",
])(
  "rejects executable or command payload %s before native admission",
  async (kind) => {
    const send = vi.fn(async () => "1");
    expect(() => decodeWorkspaceProfiles([{ kind, hidden: false }])).toThrow();
    await expect(
      openWorkspaceProfile(nonce, kind as ProfileKind, send),
    ).rejects.toThrow();
    expect(send).not.toHaveBeenCalled();
  },
);
