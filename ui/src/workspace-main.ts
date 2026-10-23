import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  type DisplayPreferences,
  displayPreferences,
} from "./features/terminal/display-preferences";
import { confirmTerminalPaste } from "./features/terminal/paste-dialog";
import {
  workspaceTerminalError,
  workspaceTerminalFailure,
} from "./features/terminal/workspace-error";
import { WorkspacePane } from "./features/terminal/workspace-pane";
import { prepareWorkspaceVisibility } from "./features/terminal/workspace-visibility";
import { formatChordSequence } from "./keybindings/chords";
import { CompositionGuard } from "./keybindings/composition";
import { KeybindingDispatcher } from "./keybindings/dispatcher";
import { classifyKeymapFocus } from "./keybindings/focus-context";
import { applyUiAccent } from "./workspace-accent";
import { WorkspaceAgentEditor } from "./workspace-agent-editor";
import { WorkspaceAppearance } from "./workspace-appearance";
import { readWorkspaceCapabilities } from "./workspace-capabilities";
import { answerCliInteractive } from "./workspace-cli-interactive";
import { installCliNewTab } from "./workspace-cli-new-tab";
import { installCliSelection } from "./workspace-cli-selection";
import {
  closeWorkspaceSession,
  confirmWorkspaceClose,
} from "./workspace-close";
import { confirmColdSsh } from "./workspace-cold-ssh";
import {
  captureWorkspaceCommands,
  type WorkspaceCommand,
  type WorkspaceCommandId,
} from "./workspace-command-catalog";
import { WorkspaceCommandPalette } from "./workspace-command-palette";
import { WorkspaceCustomProfileDialog } from "./workspace-custom-profile-dialog";
import {
  decodeCustomBriefs,
  isCustomProfileId,
  openCustomProfile,
} from "./workspace-custom-profiles";
import { workspaceDecision } from "./workspace-decision";
import { WorkspaceFingerprintDialog } from "./workspace-fingerprint-dialog";
import { historyViewAvailable } from "./workspace-history";
import { createWorkspaceHistoryEntry } from "./workspace-history-entry";
import { WorkspaceHostEditor } from "./workspace-host-editor";
import { WorkspaceHostTree } from "./workspace-host-tree";
import { readHosts } from "./workspace-hosts";
import { WorkspaceIdentityEditor } from "./workspace-identity-editor";
import { keyImportAvailable, saveIdentityKey } from "./workspace-identity-key";
import { WorkspaceKeyIdentityEditor } from "./workspace-key-identity-editor";
import {
  confirmWorkspaceKeymap,
  readWorkspaceKeymap,
  type WorkspaceKeymap,
} from "./workspace-keymap";
import { WorkspaceKeymapEditor } from "./workspace-keymap-editor";
import {
  applyUiLanguage,
  bindStaticLanguage,
  message,
  uiAttribute,
  uiContent,
  uiText,
} from "./workspace-language";
import { paneLeaves } from "./workspace-layout";
import {
  type LayoutPreference,
  loadPanePreference,
  savePanePreference,
} from "./workspace-layout-preferences";
import {
  focusedWorkspaceModal,
  WorkspaceModalEscape,
} from "./workspace-modal-escape";
import { retireBeforeReload, workspaceReloadKey } from "./workspace-navigation";
import { requestOrganization } from "./workspace-organization";
import { WorkspaceOrganizationEditor } from "./workspace-organization-editor";
import { workspacePaneDirection } from "./workspace-pane-navigation";
import { WorkspaceProfileManager } from "./workspace-profile-manager";
import type { WorkspaceProfile } from "./workspace-profiles";
import {
  loadWorkspaceProfiles,
  type ProfileKind,
  showWorkspaceProfiles,
} from "./workspace-profiles";
import { WorkspaceQuickConnectDialog } from "./workspace-quick-connect-dialog";
import { WorkspaceQuickSaveDialog } from "./workspace-quick-save-dialog";
import {
  confirmWorkspaceDocument,
  reportWorkspacePresentation,
} from "./workspace-readiness";
import { requestRecentHosts } from "./workspace-recent-hosts";
import {
  decodeWorkspaceSessions,
  openWorkspaceProfile,
} from "./workspace-sessions";
import {
  readWorkspaceSettings,
  type WorkspaceSettings,
} from "./workspace-settings";
import { WorkspaceSettingsEditor } from "./workspace-settings-editor";
import { settingsKeymapLink } from "./workspace-settings-keymap";
import { WorkspaceSplit } from "./workspace-split";
import { WorkspaceConfigDialog } from "./workspace-ssh-config-dialog";
import { WorkspaceSshDialog } from "./workspace-ssh-dialog";
import { WorkspaceSshProfileEditor } from "./workspace-ssh-profile-editor";
import {
  loadTabLabels,
  saveTabLabels,
  type TabLabel,
  tabLabel,
} from "./workspace-tab-labels";
import {
  preferredWorkspaceSession,
  rememberWorkspaceSession,
  WorkspaceTabs,
} from "./workspace-tabs";
import { readVaultStatus } from "./workspace-vault";
import { WorkspaceVaultDialog } from "./workspace-vault-dialog";
import "./workspace.css";

declare global {
  interface Window {
    __PULSE_WORKSPACE_NONCE__?: string;
    __PULSE_WORKSPACE_SSH_FINGERPRINT__?: boolean;
    __PULSE_WORKSPACE_SSH_CONNECTION__?: boolean;
  }
}
function element(id: string): HTMLElement {
  const node = document.getElementById(id);
  if (!node) throw new Error("Missing workspace element");
  return node;
}
const state = element("workspace-state"),
  profileList = element("workspace-profiles"),
  sessionList = element("workspace-sessions"),
  terminal = element("workspace-terminal"),
  caption = element("workspace-title");
const close = element("workspace-close") as HTMLButtonElement;
const duplicate = element("workspace-duplicate") as HTMLButtonElement;
const life = new AbortController();
let commandPalette: WorkspaceCommandPalette | undefined;
let hostTree: WorkspaceHostTree | undefined;
let started = false,
  busy = false,
  active: string | undefined,
  view: WorkspacePane | WorkspaceSplit | undefined;
let display: DisplayPreferences | undefined;
let cliRoutePending: Promise<void> | undefined;
let fitBusy = false;
let keymapRuntime: KeybindingDispatcher | undefined;
let keymapSource: WorkspaceKeymap | undefined;
let keymapNonce: string | undefined;
const viewIdentities = new WeakMap<object, number>();
type SessionOrigin =
  | { type: "profile"; kind: ProfileKind }
  | { type: "custom"; id: string };
const sessionOrigins = new Map<string, SessionOrigin>();
let nextViewIdentity = 0;
let catalog: readonly string[] = [];
let labels = new Map<string, TabLabel>();
installCliNewTab(window, {
  signal: life.signal,
  current: () => window.__PULSE_WORKSPACE_NONCE__,
  available: () =>
    !busy &&
    !fitBusy &&
    profileList.dataset.state === "ready" &&
    !document.querySelector("dialog[open]"),
  captureSplit: () => {
    if (!active || terminal.dataset.state !== "ready") return;
    if (view instanceof WorkspacePane)
      return { session: active, identity: view };
    if (view instanceof WorkspaceSplit && view.canExtend && view.layoutIdentity)
      return { session: active, identity: view.layoutIdentity };
  },
  subscribeInteractive: (receive) =>
    listen("pulseterm:cli-interactive", (event) => receive(event.payload)),
  interactive: (prompt, signal) =>
    answerCliInteractive(document, prompt, signal),
  respondInteractive: async (nonce, token, round, answers) => {
    await invoke("workspace_ssh_interactive_session_answer", {
      nonce,
      token,
      round,
      answers,
    });
  },
  cancelInteractive: async (nonce, token) => {
    await invoke("workspace_ssh_session_cancel", { nonce, token });
  },
  confirmWindow: (signal) =>
    workspaceDecision(
      document,
      {
        title: "允许 CLI 新建窗口？",
        description:
          "创建一个空工作区。已有终端保留在原窗口；新窗口启动终端时会另行确认。",
        preview: message("空工作区"),
        previewLabel: "新窗口",
        accept: "新建窗口",
        cancel: "取消",
      },
      signal,
    ),
  confirm: (profile, signal, direction, baseDirection, cwd) =>
    workspaceDecision(
      document,
      {
        title: baseDirection
          ? "为冷分屏创建首个终端？"
          : direction
            ? "允许 CLI 创建分屏终端？"
            : "允许 CLI 创建本地终端？",
        description: cwd
          ? "仅本次使用 CLI 的当前工作目录，不修改保存的 Profile。取消或未确认不会启动。"
          : baseDirection
            ? "先创建并呈现一个本地终端，再单独确认第二个分屏终端。取消第二步会保留首个会话；取消此步不会启动。"
            : "CLI 请求启动已保存或后端发现的本地 Profile。确认后创建一个保留会话；取消或未确认不会启动。已有会话不受影响。",
        preview: cwd
          ? `${profile}\n${cwd}`
          : direction || baseDirection
            ? message(
                "{0} · {1}",
                profile,
                message(
                  (direction ?? baseDirection) === "row"
                    ? "左右分屏"
                    : "上下分屏",
                ),
              )
            : profile,
        previewLabel: "本地 Profile",
        accept: baseDirection
          ? "创建首个终端"
          : direction
            ? "创建分屏"
            : "创建终端",
        cancel: "取消",
      },
      signal,
    ),
  confirmColdSsh: (selection, signal) =>
    confirmColdSsh(document, selection, signal, {
      read: (stop) =>
        readVaultStatus(window.__PULSE_WORKSPACE_NONCE__ ?? "", stop),
      unlock: (stop) => vaultDialog.openAndWait(stop),
    }),
  confirmSsh: (selection, signal) =>
    workspaceDecision(
      document,
      {
        title: "允许 CLI 连接保存的 SSH 主机？",
        description:
          "仅使用请求时已确认的 Vault 密钥和保存目标。仍须通过该连接的指纹验证；取消不会认证或创建会话。",
        preview:
          selection.authentication === "password-interactive"
            ? message(
                "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\n密码 + MFA（两种因素均须成功）",
                selection.hostLabel,
                selection.address,
                selection.port,
                selection.identityLabel,
                selection.username,
                selection.hostId,
                selection.identityId,
              )
            : selection.authentication === "agent-interactive"
              ? message(
                  "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\nAgent + MFA（两种因素均须成功）",
                  selection.hostLabel,
                  selection.address,
                  selection.port,
                  selection.identityLabel,
                  selection.username,
                  selection.hostId,
                  selection.identityId,
                )
              : selection.authentication === "key-interactive"
                ? message(
                    "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\n私钥 + MFA（两种因素均须成功）",
                    selection.hostLabel,
                    selection.address,
                    selection.port,
                    selection.identityLabel,
                    selection.username,
                    selection.hostId,
                    selection.identityId,
                  )
                : selection.authentication === "interactive"
                  ? message(
                      "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}\n交互认证 / MFA（不读取保存密码）",
                      selection.hostLabel,
                      selection.address,
                      selection.port,
                      selection.identityLabel,
                      selection.username,
                      selection.hostId,
                      selection.identityId,
                    )
                  : message(
                      "{0} · {1}:{2}\n{3} · {4}\n主机 {5}\n身份 {6}",
                      selection.hostLabel,
                      selection.address,
                      selection.port,
                      selection.identityLabel,
                      selection.username,
                      selection.hostId,
                      selection.identityId,
                    ),
        previewLabel: "保存的主机与身份",
        accept: "连接",
        cancel: "取消",
      },
      signal,
    ),
  answer: (nonce, ticket, accept) =>
    invoke("workspace_cli_answer", { nonce, ticket, accept }),
  opened: async (nonce, id, layout) => {
    if (cliRoutePending) await cliRoutePending;
    if (busy || fitBusy || life.signal.aborted)
      throw new Error("workspace.cli_busy");
    busy = true;
    controls();
    try {
      await listSessions(nonce);
      await refreshRecentHosts(nonce);
      if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
        return;
      if (layout) {
        const identity =
          view instanceof WorkspaceSplit ? view.layoutIdentity : view;
        if (
          identity !== layout.target.identity ||
          active !== layout.target.session
        )
          throw new Error("workspace.cli_stale_target");
        busy = false;
        if (!(await showSplit(layout.direction, id, layout.target.session)))
          throw new Error("workspace.cli_split_failed");
        if (view instanceof WorkspaceSplit && !view.focusSession(id))
          throw new Error("workspace.cli_split_focus_unconfirmed");
        rememberWorkspaceSession(id, layoutStorage());
      } else {
        await attach(nonce, id);
        if (!(view instanceof WorkspacePane) || !view.focusOutput())
          throw new Error("workspace.cli_input_unconfirmed");
      }
    } finally {
      busy = false;
      controls();
    }
  },
  admitPresentation: async (nonce, ticket, session) => {
    const result = await invoke<unknown>("workspace_cli_select_admit", {
      nonce,
      ticket,
      session,
    });
    if (result !== null) throw Error("workspace.cli_selection_unconfirmed");
  },
  completePresentation: async (nonce, ticket, session, selected) => {
    const result = await invoke<unknown>("workspace_cli_selected", {
      nonce,
      ticket,
      session,
      selected,
    });
    if (result !== null) throw Error("workspace.cli_selection_unconfirmed");
  },
  failed: (error) => {
    uiText(
      state,
      workspaceTerminalFailure(
        error,
        "CLI 启动或显示未确认 · 未重试，请刷新检查保留的会话",
      ),
    );
  },
});
installCliSelection(window, {
  signal: life.signal,
  current: () => window.__PULSE_WORKSPACE_NONCE__,
  available: () =>
    cliRoutePending !== undefined ||
    (!busy &&
      !fitBusy &&
      profileList.dataset.state === "ready" &&
      !document.querySelector("dialog[open]")),
  admit: async ({ nonce, ticket, session }) => {
    const result = await invoke<unknown>("workspace_cli_select_admit", {
      nonce,
      ticket,
      session,
    });
    if (result !== null) throw Error("workspace.cli_selection_unconfirmed");
  },
  select: async ({ nonce, session }, signal) => {
    if (cliRoutePending) await cliRoutePending;
    const ids = await listSessions(nonce);
    if (
      !ids.includes(session) ||
      signal.aborted ||
      life.signal.aborted ||
      window.__PULSE_WORKSPACE_NONCE__ !== nonce ||
      document.querySelector("dialog[open]")
    )
      return false;
    return selectSession(nonce, session, signal);
  },
  complete: async ({ nonce, ticket, session }, selected) => {
    const result = await invoke<unknown>("workspace_cli_selected", {
      nonce,
      ticket,
      session,
      selected,
    });
    if (result !== null) throw Error("workspace.cli_selection_unconfirmed");
  },
});
let labelsLoaded = false;
let editTarget: string | undefined;
const labelDialog = element("workspace-label-editor") as HTMLDialogElement;
const labelTitle = element("workspace-label-title") as HTMLInputElement;
const labelColor = element("workspace-label-color") as HTMLSelectElement;
function stopLabelEditor(): void {
  editTarget = undefined;
  if (labelDialog.open) labelDialog.close();
}
life.signal.addEventListener("abort", stopLabelEditor, { once: true });
function layoutStorage(): Storage | undefined {
  try {
    return window.sessionStorage;
  } catch {
    return undefined;
  }
}
const tabs = new WorkspaceTabs(element("workspace-tabs"), terminal, (id) => {
  const nonce = window.__PULSE_WORKSPACE_NONCE__;
  if (nonce) void selectSession(nonce, id);
});
// Accent changes UI tokens only; terminal base palettes remain frozen per session.
for (const root of document.querySelectorAll<HTMLElement>(
  "title, .workspace-header, .workspace-sidebar, .workspace-terminal-header, #workspace-settings-dialog, #workspace-command-dialog, #workspace-keymap-dialog, #workspace-host-dialog, #workspace-ssh-profile-dialog, #workspace-ssh-dialog, #workspace-identity-dialog, #workspace-agent-dialog, #workspace-vault-dialog, #workspace-profile-dialog, #workspace-organization-dialog, #workspace-fingerprint-dialog, #workspace-config-dialog, #workspace-quick-connect-dialog, #workspace-quick-save-dialog",
))
  bindStaticLanguage(root);
const appearance = new WorkspaceAppearance(document, life.signal);
function acceptSettings(value: WorkspaceSettings): boolean {
  const languageApplied = applyUiLanguage(
    document,
    value.values.ui_language?.value ?? "zh-CN",
    life.signal,
  );
  const applied = appearance.apply(
    value.values.ui_appearance?.value ?? "light",
  );
  const accentApplied = applyUiAccent(
    document,
    value.values.ui_accent?.value ?? "cyan",
    life.signal,
  );
  display = displayPreferences({
    fontSize: value.values.font_size.value,
    lineHeightPercent: value.values.line_height_percent?.value ?? 100,
    letterSpacing: value.values.letter_spacing?.value ?? 0,
    fontFamily: value.values.font_family.value,
    ...(value.values.font_fallback
      ? { fontFallback: value.values.font_fallback.value }
      : {}),
    ...(value.values.font_ligatures
      ? { fontLigatures: value.values.font_ligatures.value }
      : {}),
    cursorBlink: value.values.cursor_blink.value,
    copyOnSelect: value.values.copy_on_select.value,
    rightClickPaste: value.values.right_click_paste.value,
    confirmClose: value.values.confirm_close?.value ?? true,
    scrollOnInput: value.values.scroll_on_input?.value ?? true,
    scrollOnOutput: value.values.scroll_on_output?.value ?? false,
    macOptionIsMeta: value.values.mac_option_as_meta?.value ?? false,
    backspaceSend: value.values.backspace_send?.value ?? "del",
    deleteSend: value.values.delete_send?.value ?? "csi",
  });
  return applied && accentApplied && languageApplied;
}
const settingsEditor = new WorkspaceSettingsEditor({
  dialog: element("workspace-settings-dialog") as HTMLDialogElement,
  fields: element("workspace-settings-fields"),
  navigation: element("workspace-settings-navigation"),
  openKeymap: () => {
    if (!busy && !fitBusy) void openKeymapSettings();
  },
  status: element("workspace-settings-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  changed: acceptSettings,
  applyAppearance: (mode) => appearance.apply(mode),
  applyAccent: (value) => applyUiAccent(document, value, life.signal),
});
const fingerprintDialog = new WorkspaceFingerprintDialog({
  dialog: element("workspace-fingerprint-dialog") as HTMLDialogElement,
  fields: element("workspace-fingerprint-fields"),
  status: element("workspace-fingerprint-status"),
  cancel: element("workspace-fingerprint-cancel") as HTMLButtonElement,
  approve: element("workspace-fingerprint-approve") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
});
const configDialog = new WorkspaceConfigDialog({
  dialog: element("workspace-config-dialog") as HTMLDialogElement,
  form: element("workspace-config-form") as HTMLFormElement,
  alias: element("workspace-config-alias") as HTMLInputElement,
  read: element("workspace-config-read") as HTMLButtonElement,
  confirm: element("workspace-config-confirm") as HTMLButtonElement,
  fields: element("workspace-config-fields"),
  status: element("workspace-config-status"),
  importControls: {
    label: element("workspace-config-label") as HTMLInputElement,
    prepare: element("workspace-config-prepare") as HTMLButtonElement,
    save: element("workspace-config-save") as HTMLButtonElement,
  },
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  enabled: () => window.__PULSE_WORKSPACE_SSH_FINGERPRINT__ === true,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  restoreFocus: () => element("workspace-host-open").focus(),
});
hostTree = new WorkspaceHostTree({
  root: element("workspace-host-tree"),
  signal: life.signal,
  enabled: () =>
    window.__PULSE_WORKSPACE_SSH_CONNECTION__ === true &&
    !busy &&
    !fitBusy &&
    !life.signal.aborted &&
    !document.querySelector("dialog[open]"),
  select: (id) => {
    void sshDialog.open(id);
  },
});
const organizationEditor = new WorkspaceOrganizationEditor({
  changed: (value) => hostTree?.update(value),
  dialog: element("workspace-organization-dialog") as HTMLDialogElement,
  fields: element("workspace-organization-fields"),
  status: element("workspace-organization-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  restoreFocus: () => element("workspace-organization-open").focus(),
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
});
const agentEditor = new WorkspaceAgentEditor({
  dialog: element("workspace-agent-dialog") as HTMLDialogElement,
  fields: element("workspace-agent-fields"),
  status: element("workspace-agent-status"),
  done: element("workspace-agent-done") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
});
element("workspace-agent-open").addEventListener(
  "click",
  () => {
    if (!busy && !fitBusy && !document.querySelector("dialog[open]"))
      void agentEditor.open();
  },
  { signal: life.signal },
);
const sshProfileEditor = new WorkspaceSshProfileEditor({
  dialog: element("workspace-ssh-profile-dialog") as HTMLDialogElement,
  fields: element("workspace-ssh-profile-fields"),
  status: element("workspace-ssh-profile-status"),
  done: element("workspace-ssh-profile-done") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  enabled: () =>
    !busy &&
    !fitBusy &&
    profileList.dataset.state === "ready" &&
    !document.querySelector("dialog[open]"),
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  changed: (value) => hostTree?.updateHosts(value),
});
element("workspace-ssh-profile-open").addEventListener(
  "click",
  () => {
    void sshProfileEditor.open();
  },
  { signal: life.signal },
);
const hostEditor = new WorkspaceHostEditor({
  changed: (value) => hostTree?.updateHosts(value),
  fingerprintEnabled: () => window.__PULSE_WORKSPACE_SSH_FINGERPRINT__ === true,
  fingerprint: (host) => {
    void fingerprintDialog.open(host);
  },
  dialog: element("workspace-host-dialog") as HTMLDialogElement,
  done: element("workspace-host-done") as HTMLButtonElement,
  fields: element("workspace-host-fields"),
  status: element("workspace-host-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
});
const identityEditorOptions = {
  dialog: element("workspace-identity-dialog") as HTMLDialogElement,
  done: element("workspace-identity-done") as HTMLButtonElement,
  fields: element("workspace-identity-fields"),
  status: element("workspace-identity-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value: boolean) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
};
const identityEditor = new WorkspaceIdentityEditor(identityEditorOptions);
const keyIdentityEditor = new WorkspaceKeyIdentityEditor({
  ...identityEditorOptions,
  imports: { available: keyImportAvailable, save: saveIdentityKey },
});
const vaultDialog = new WorkspaceVaultDialog({
  dialog: element("workspace-vault-dialog") as HTMLDialogElement,
  fields: element("workspace-vault-fields"),
  status: element("workspace-vault-status"),
  done: element("workspace-vault-done") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
});
const profileManager = new WorkspaceProfileManager({
  dialog: element("workspace-profile-dialog") as HTMLDialogElement,
  list: element("workspace-profile-preferences"),
  status: element("workspace-profile-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  defaults: {
    root: element("workspace-profile-default"),
    native: {
      read: (nonce) => invoke("workspace_profile_default_read", { nonce }),
      save: (nonce, ticket, selected) =>
        invoke("workspace_profile_default_save", { nonce, ticket, selected }),
      cancel: (nonce, ticket) =>
        invoke("workspace_profile_default_cancel", { nonce, ticket }),
    },
    launch: (nonce, kind) => {
      if (isCustomProfileId(kind)) void startCustomProfile(nonce, kind);
      else void startProfile(nonce, kind);
    },
  },
  editor: {
    root: element("workspace-profile-editor"),
    native: {
      read: (nonce, shell) =>
        invoke("workspace_profile_editor_read", { nonce, shell }),
      save: (nonce, ticket, config) =>
        invoke("workspace_profile_editor_save", { nonce, ticket, config }),
      cancel: (nonce, ticket) =>
        invoke("workspace_profile_editor_cancel", { nonce, ticket }),
    },
  },
  lifecycle: {
    root: element("workspace-profile-lifecycle"),
    native: {
      deleted: (nonce) => invoke("workspace_profile_deleted", { nonce }),
      prepare: (nonce, shell, action) =>
        invoke("workspace_profile_lifecycle_prepare", { nonce, shell, action }),
      commit: (nonce, ticket) =>
        invoke("workspace_profile_lifecycle_commit", { nonce, ticket }),
      cancel: (nonce, ticket) =>
        invoke("workspace_profile_lifecycle_cancel", { nonce, ticket }),
    },
  },
  save: (nonce, profile, hidden) =>
    invoke("workspace_profile_visibility", { nonce, profile, hidden }),
  refresh: async (): Promise<readonly WorkspaceProfile[]> => {
    const nonce = window.__PULSE_WORKSPACE_NONCE__;
    if (!nonce || life.signal.aborted) throw Error("workspace.denied");
    const result = await loadWorkspaceProfiles(
      nonce,
      (value) => invoke("workspace_profiles", { nonce: value }),
      life.signal,
    );
    if (result.status !== "ready") {
      profileList.dataset.state = "failed";
      controls();
      throw Error("workspace.unavailable");
    }
    showWorkspaceProfiles(profileList, result.profiles, (kind) => {
      void startProfile(nonce, kind);
    });
    profileList.dataset.state = "ready";
    await refreshCustomProfiles(nonce);
    return result.profiles;
  },
  working: (value) => {
    busy = value;
    controls();
  },
});
const customProfileDialog = new WorkspaceCustomProfileDialog({
  dialog: element("workspace-custom-profile-dialog") as HTMLDialogElement,
  list: element("workspace-custom-profile-entries"),
  editor: element("workspace-custom-profile-editor"),
  status: element("workspace-custom-profile-status"),
  done: element("workspace-custom-profile-done") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  native: {
    read: (nonce) => invoke("workspace_custom_profile_read", { nonce }),
    save: (nonce, ticket, change) =>
      invoke("workspace_custom_profile_save", { nonce, ticket, change }),
    cancel: (nonce, ticket) =>
      invoke("workspace_custom_profile_cancel", { nonce, ticket }),
  },
  working: (value) => {
    busy = value;
    controls();
  },
  changed: () => {
    const nonce = window.__PULSE_WORKSPACE_NONCE__;
    return nonce ? refreshCustomProfiles(nonce) : undefined;
  },
  start: (id) => {
    const nonce = window.__PULSE_WORKSPACE_NONCE__;
    if (nonce) void startCustomProfile(nonce, id);
  },
  returned: () => profileManager.open(),
});
element("workspace-custom-profile-manage").addEventListener(
  "click",
  () => {
    if (busy || fitBusy) return;
    profileManager.close();
    customProfileDialog.open();
  },
  { signal: life.signal },
);
let customListVersion = 0;
async function refreshCustomProfiles(nonce: string): Promise<void> {
  const root = element("workspace-custom-profiles"),
    version = ++customListVersion;
  let timer: ReturnType<typeof setTimeout> | undefined;
  try {
    const profiles = decodeCustomBriefs(
      await Promise.race([
        invoke("workspace_custom_profile_list", { nonce }),
        new Promise<never>((_, reject) => {
          timer = setTimeout(
            () => reject(Error("workspace.unavailable")),
            5000,
          );
        }),
      ]),
    );
    if (
      life.signal.aborted ||
      nonce !== window.__PULSE_WORKSPACE_NONCE__ ||
      version !== customListVersion
    )
      return;
    root.replaceChildren();
    for (const profile of profiles.filter((profile) => !profile.hidden)) {
      const row = root.ownerDocument.createElement("li"),
        button = root.ownerDocument.createElement("button");
      button.type = "button";
      button.textContent = profile.name;
      button.dataset.customProfile = profile.id;
      button.addEventListener(
        "click",
        () => {
          void startCustomProfile(nonce, profile.id);
        },
        { signal: life.signal },
      );
      row.append(button);
      root.append(row);
    }
    root.dataset.state = "ready";
  } catch {
    if (
      !life.signal.aborted &&
      nonce === window.__PULSE_WORKSPACE_NONCE__ &&
      version === customListVersion
    ) {
      uiText(root, "自定义 Profile 读取失败，请重新读取。");
      root.dataset.state = "failed";
    }
  } finally {
    clearTimeout(timer);
    controls();
  }
}
const quickSave = new WorkspaceQuickSaveDialog({
  dialog: element("workspace-quick-save-dialog") as HTMLDialogElement,
  label: element("workspace-quick-save-label") as HTMLInputElement,
  description: element("workspace-quick-save-target"),
  status: element("workspace-quick-save-status"),
  save: element("workspace-quick-save-submit") as HTMLButtonElement,
  cancel: element("workspace-quick-save-cancel") as HTMLButtonElement,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  saved: async (nonce) => {
    try {
      const catalogue = await readHosts(nonce, life.signal);
      if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce) {
        hostTree?.updateHosts(catalogue);
        await refreshRecentHosts(nonce);
      }
    } catch {
      if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
        uiText(state, "主机列表更新未确认，请刷新查看。已有连接继续运行。");
    }
  },
});
const sshDialog = new WorkspaceSshDialog({
  dialog: element("workspace-ssh-dialog") as HTMLDialogElement,
  host: element("workspace-ssh-host") as HTMLSelectElement,
  fingerprint: element("workspace-ssh-fingerprint") as HTMLButtonElement,
  fingerprintEnabled: () => window.__PULSE_WORKSPACE_SSH_FINGERPRINT__ === true,
  inspectFingerprint: (host) => {
    void fingerprintDialog.open(host);
  },
  identity: element("workspace-ssh-identity") as HTMLSelectElement,
  connect: element("workspace-ssh-connect") as HTMLButtonElement,
  cancel: element("workspace-ssh-cancel") as HTMLButtonElement,
  status: element("workspace-ssh-status"),
  selectedHost: element("workspace-ssh-selected-host"),
  temporaryConnected: (nonce, target) => quickSave.open(nonce, target),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  connected: async (nonce, id) => {
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    busy = true;
    controls();
    try {
      await listSessions(nonce);
      await refreshRecentHosts(nonce);
      if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
        await attach(nonce, id);
    } catch (error) {
      if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
        uiText(
          state,
          workspaceTerminalFailure(
            error,
            "SSH 会话显示未确认 · 会话保留，请刷新检查，未重复连接",
          ),
        );
    } finally {
      if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce) {
        busy = false;
        controls();
      }
    }
  },
});
window.addEventListener(
  "pulseterm:cli-ssh-route",
  (event) => {
    const body: unknown = (event as CustomEvent<unknown>).detail;
    if (!body || typeof body !== "object" || Array.isArray(body)) return;
    const value = body as Record<string, unknown>;
    if (
      Object.keys(value).length !== 3 ||
      value.version !== 1 ||
      typeof value.nonce !== "string" ||
      value.nonce !== window.__PULSE_WORKSPACE_NONCE__ ||
      typeof value.token !== "string" ||
      !/^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/.test(
        value.token,
      ) ||
      life.signal.aborted ||
      cliRoutePending
    )
      return;
    const pending = sshDialog.continueCliRoute(value.nonce, value.token);
    cliRoutePending = pending;
    void pending
      .catch(() => {
        if (
          !life.signal.aborted &&
          value.nonce === window.__PULSE_WORKSPACE_NONCE__
        )
          uiText(state, "CLI 路线已取消或失效，请检查保留会话，未重新连接。");
      })
      .finally(() => {
        if (cliRoutePending === pending) cliRoutePending = undefined;
      });
  },
  { signal: life.signal },
);
const quickConnect = new WorkspaceQuickConnectDialog({
  dialog: element("workspace-quick-connect-dialog") as HTMLDialogElement,
  address: element("workspace-quick-connect-address") as HTMLInputElement,
  label: element("workspace-quick-connect-label") as HTMLInputElement,
  inspect: element("workspace-quick-connect-inspect") as HTMLButtonElement,
  continue: element("workspace-quick-connect-continue") as HTMLButtonElement,
  temporary: element("workspace-quick-connect-temporary") as HTMLButtonElement,
  cancel: element("workspace-quick-connect-cancel") as HTMLButtonElement,
  preview: element("workspace-quick-connect-preview"),
  status: element("workspace-quick-connect-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  enabled: () =>
    window.__PULSE_WORKSPACE_SSH_CONNECTION__ === true &&
    !busy &&
    !fitBusy &&
    profileList.dataset.state === "ready" &&
    !document.querySelector("dialog[open]"),
  working: (value) => {
    if (!life.signal.aborted) {
      busy = value;
      controls();
    }
  },
  changed: (value) => hostTree?.updateHosts(value),
  select: (id) => {
    void sshDialog.open(id);
  },
  selectTemporary: (nonce, target) => {
    if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
      void sshDialog.open(target.target.id, target);
  },
});
element("workspace-quick-connect-open").addEventListener(
  "click",
  () => {
    void quickConnect.open();
  },
  { signal: life.signal },
);
async function refreshRecentHosts(nonce: string): Promise<void> {
  try {
    const recent = await requestRecentHosts(nonce, life.signal);
    if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
      hostTree?.updateRecent(recent);
  } catch {
    if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
      hostTree?.updateRecent(undefined);
  }
}
function controls(): void {
  hostTree?.updateEnabled();
  const configOpen = element("workspace-config-open") as HTMLButtonElement;
  configOpen.hidden = window.__PULSE_WORKSPACE_SSH_FINGERPRINT__ !== true;
  configOpen.disabled =
    busy ||
    fitBusy ||
    profileList.dataset.state !== "ready" ||
    configOpen.hidden;
  const quickOpen = element(
    "workspace-quick-connect-open",
  ) as HTMLButtonElement;
  quickOpen.hidden = window.__PULSE_WORKSPACE_SSH_CONNECTION__ !== true;
  quickOpen.disabled =
    quickOpen.hidden ||
    busy ||
    fitBusy ||
    profileList.dataset.state !== "ready";
  const sshOpen = element("workspace-ssh-open") as HTMLButtonElement;
  sshOpen.hidden = window.__PULSE_WORKSPACE_SSH_CONNECTION__ !== true;
  sshOpen.disabled =
    busy || fitBusy || profileList.dataset.state !== "ready" || sshOpen.hidden;
  (element("workspace-vault-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  (element("workspace-identity-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  (element("workspace-key-identity-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  const agentOpen = element("workspace-agent-open") as HTMLButtonElement;
  agentOpen.hidden = window.__PULSE_WORKSPACE_SSH_CONNECTION__ !== true;
  agentOpen.disabled =
    agentOpen.hidden ||
    busy ||
    fitBusy ||
    profileList.dataset.state !== "ready";
  (element("workspace-organization-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  (element("workspace-ssh-profile-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  (element("workspace-host-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  for (const button of document.querySelectorAll<HTMLButtonElement>(
    "#workspace-profiles button, #workspace-custom-profiles button, #workspace-sessions button",
  ))
    button.disabled = busy || fitBusy;
  (element("workspace-profile-manage") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  const ratio = element("workspace-split-ratio") as HTMLInputElement;
  element("workspace-ratio-controls").hidden = !(
    view instanceof WorkspaceSplit
  );
  ratio.disabled =
    busy ||
    fitBusy ||
    !(view instanceof WorkspaceSplit) ||
    !view.canAdjustRatio;
  if (view instanceof WorkspaceSplit) {
    ratio.value = String(view.ratio);
    element("workspace-split-percent").textContent = `${view.ratio}%`;
  }
  close.disabled = busy || fitBusy || !active;
  for (const button of element(
    "workspace-split-controls",
  ).querySelectorAll<HTMLButtonElement>("button"))
    button.disabled =
      busy ||
      fitBusy ||
      !active ||
      (view instanceof WorkspaceSplit && !view.canAdjustRatio);
  const maximize = element("workspace-pane-maximize") as HTMLButtonElement;
  const single = element("workspace-pane-single") as HTMLButtonElement;
  maximize.hidden = single.hidden = !(view instanceof WorkspaceSplit);
  maximize.disabled =
    busy ||
    fitBusy ||
    !(view instanceof WorkspaceSplit) ||
    !view.canChangeLayout;
  uiText(
    maximize,
    view instanceof WorkspaceSplit && view.isMaximized
      ? "还原分屏"
      : "最大化窗格",
  );
  maximize.setAttribute(
    "aria-pressed",
    String(view instanceof WorkspaceSplit && view.isMaximized),
  );
  single.disabled = busy || fitBusy || !(view instanceof WorkspaceSplit);
  const target = element("workspace-split-target") as HTMLSelectElement;
  if (target.value === active)
    target.value =
      catalog.find(
        (id) =>
          id !== active &&
          (!(view instanceof WorkspaceSplit) || !view.hasSession(id)),
      ) ??
      catalog.find((id) => id !== active) ??
      "";
  target.disabled = busy || fitBusy;
  duplicate.disabled =
    busy ||
    fitBusy ||
    !active ||
    !sessionOrigins.has(
      view instanceof WorkspaceSplit ? view.focusedSession : (active ?? ""),
    );
  (element("workspace-rename") as HTMLButtonElement).disabled =
    busy || fitBusy || !active;
  (element("workspace-history") as HTMLButtonElement).disabled =
    busy || fitBusy || !active || !historyViewAvailable(terminal.dataset.state);
  tabs.render(catalog, active, busy || fitBusy, labels);
  (element("workspace-command-open") as HTMLButtonElement).disabled =
    busy || fitBusy || profileList.dataset.state !== "ready";
  commandPalette?.check();
}
async function listSessions(nonce: string): Promise<readonly string[]> {
  const ids = decodeWorkspaceSessions(
    await invoke("workspace_sessions", { nonce }),
  );
  if (life.signal.aborted) return [];
  catalog = ids;
  if (!labelsLoaded) {
    labels = new Map(loadTabLabels(ids, layoutStorage()));
    labelsLoaded = true;
  } else
    for (const id of labels.keys()) if (!ids.includes(id)) labels.delete(id);
  const choices = element("workspace-split-target") as HTMLSelectElement;
  choices.replaceChildren();
  for (const id of ids) {
    const option = document.createElement("option");
    option.value = id;
    uiText(option, message("会话 {0}", id));
    choices.append(option);
  }
  element("workspace-split-controls").hidden = ids.length < 2;
  sessionList.replaceChildren();
  for (const id of ids) {
    const row = document.createElement("li"),
      button = document.createElement("button");
    button.type = "button";
    uiText(button, message("会话 {0}", id));
    button.dataset.session = id;
    button.addEventListener("click", () => {
      void selectSession(nonce, id);
    });
    row.append(button);
    sessionList.append(row);
  }
  controls();
  return ids;
}
async function attach(nonce: string, id: string): Promise<void> {
  savePanePreference(undefined, layoutStorage());
  view?.dispose();
  view = undefined;
  active = id;
  rememberWorkspaceSession(id, layoutStorage());
  terminal.replaceChildren();
  terminal.classList.remove("workspace-layout");
  uiText(caption, message("本地会话 {0}", id));
  let lastPasteNotice: string | undefined;
  const next = new WorkspacePane({
    nonce,
    session: id,
    input: true,
    display,
    element: terminal,
    signal: life.signal,
    paste: {
      confirm: (text, assessment, signal) =>
        confirmTerminalPaste(terminal.ownerDocument, text, assessment, signal),
      rejected: (code) => {
        if (life.signal.aborted || active !== id) return;
        const messages: Record<string, string> = {
          "paste.size": "未粘贴：单次内容最多 1012 UTF-8 字节",
          "paste.unsupported_control": "未粘贴：包含不支持的控制字符",
          "paste.invalid_unicode": "未粘贴：无效 Unicode",
          "paste.busy": "已有粘贴等待确认，请先处理",
          "paste.cancelled": "已取消粘贴，未发送",
          "paste.unavailable": "未粘贴：通路不可用，不自动重试",
        };
        uiText(state, messages[code] ?? "未粘贴");
        lastPasteNotice = uiContent(state).source;
      },
      submitted: () => {
        if (life.signal.aborted || active !== id) return;
        if (
          lastPasteNotice !== undefined &&
          uiContent(state).source === lastPasteNotice
        )
          uiText(state, "终端已连接 · 多行粘贴需要确认");
        lastPasteNotice = undefined;
      },
    },
    phase: (phase, error) => {
      if (life.signal.aborted || active !== id || phase === "disposed") return;
      terminal.dataset.state = phase;
      fitBusy =
        phase === "loading" || phase === "resizing" || phase === "paused";
      keymapRuntime?.cancelPrefix();
      const text: Record<string, string> = {
        loading: "正在恢复终端…",
        resizing: "正在等待原生尺寸确认…",
        ready: "终端已连接 · 多行粘贴需要确认",
        paused: "终端视图已暂停 · 会话已保留",
        ended: "会话输出已结束",
        failed: "终端连接失败 · 会话仍保留，可重新选择恢复",
      };
      uiText(
        state,
        error
          ? workspaceTerminalFailure(error, text[phase] ?? "")
          : (text[phase] ?? ""),
      );
      if (error) {
        const code = workspaceTerminalError(error);
        terminal.dataset.failure = code;
        void invoke("workspace_terminal_report_failure", { nonce, code }).catch(
          () => {},
        );
      }
      controls();
    },
  });
  view = next;
  await next.mount();
}
async function showSplit(
  direction: "row" | "column",
  peer?: string,
  expectedFirst?: string,
  restored?: LayoutPreference,
): Promise<boolean> {
  const nonce = window.__PULSE_WORKSPACE_NONCE__,
    first =
      restored?.focused ??
      (view instanceof WorkspaceSplit ? view.focusedSession : active),
    second =
      peer ??
      (restored
        ? paneLeaves(restored.tree).find(
            (leaf) => leaf.view.sessionId !== first,
          )?.view.sessionId
        : undefined) ??
      (element("workspace-split-target") as HTMLSelectElement).value;
  if (
    !nonce ||
    !first ||
    first === second ||
    (expectedFirst !== undefined &&
      (first !== expectedFirst ||
        (!(view instanceof WorkspacePane) &&
          !(view instanceof WorkspaceSplit)))) ||
    busy ||
    fitBusy ||
    life.signal.aborted ||
    !catalog.includes(first) ||
    !catalog.includes(second)
  )
    return false;
  if (view instanceof WorkspaceSplit && !restored) {
    const current = view;
    busy = true;
    controls();
    try {
      await current.addSession(second, direction, catalog);
      return true;
    } catch {
      if (!life.signal.aborted)
        uiText(state, "无法分屏 · 请选尚未显示的会话或直接相邻窗格");
      return false;
    } finally {
      busy = false;
      controls();
    }
  }
  busy = true;
  controls();
  view?.dispose();
  view = undefined;
  terminal.replaceChildren();
  terminal.classList.add("workspace-layout");
  try {
    const next: WorkspaceSplit = new WorkspaceSplit({
      restored,
      nonce,
      first,
      second,
      direction,
      catalog,
      display,
      element: terminal,
      signal: life.signal,
      paste: {
        confirm: (text, assessment, signal) =>
          confirmTerminalPaste(
            terminal.ownerDocument,
            text,
            assessment,
            signal,
          ),
      },
      changed: (phase, focused, error) => {
        if (life.signal.aborted || view !== next) return;
        active = focused;
        uiText(caption, message("会话 {0}", focused));
        rememberWorkspaceSession(focused, layoutStorage());
        savePanePreference(next.preference, layoutStorage());
        terminal.dataset.state = phase;
        fitBusy =
          phase === "loading" || phase === "resizing" || phase === "paused";
        keymapRuntime?.cancelPrefix();
        uiText(
          state,
          phase === "paused"
            ? "分屏视图已暂停 · 会话已保留"
            : phase === "ended"
              ? "分屏输出已结束"
              : message("分屏 · 焦点会话 {0}", focused),
        );
        if (error) {
          const code = workspaceTerminalError(error);
          terminal.dataset.failure = code;
          uiText(
            state,
            workspaceTerminalFailure(error, "分屏显示失败 · 原会话仍保留"),
          );
          void invoke("workspace_terminal_report_failure", {
            nonce,
            code,
          }).catch(() => {});
        }
        controls();
      },
    });
    view = next;
    await next.mount();
    return true;
  } catch (error) {
    if (!life.signal.aborted)
      uiText(
        state,
        workspaceTerminalFailure(
          error,
          "无法显示分屏 · 原会话仍保留，可重新选择",
        ),
      );
    return false;
  } finally {
    busy = false;
    controls();
  }
}

async function selectSession(
  nonce: string,
  id: string,
  signal?: AbortSignal,
): Promise<boolean> {
  if (
    busy ||
    fitBusy ||
    life.signal.aborted ||
    signal?.aborted ||
    window.__PULSE_WORKSPACE_NONCE__ !== nonce
  )
    return false;
  if (view instanceof WorkspaceSplit && view.hasSession(id)) {
    keymapRuntime?.cancelPrefix();
    if (view.isMaximized && view.focusedSession !== id) {
      if (!view.canChangeLayout) return false;
      await view.toggleMaximized();
    }
    return view.focusSession(id);
  }
  if (
    active === id &&
    view instanceof WorkspacePane &&
    (terminal.dataset.state === "ready" || terminal.dataset.state === "ended")
  )
    return view.focusOutput();
  busy = true;
  controls();
  try {
    await attach(nonce, id);
    if (
      life.signal.aborted ||
      signal?.aborted ||
      window.__PULSE_WORKSPACE_NONCE__ !== nonce ||
      active !== id ||
      !(view instanceof WorkspacePane)
    )
      return false;
    return view.focusOutput();
  } catch (error) {
    if (!life.signal.aborted)
      uiText(
        state,
        workspaceTerminalFailure(
          error,
          "无法恢复显示 · 会话仍保留，可重新选择",
        ),
      );
    return false;
  } finally {
    busy = false;
    controls();
  }
}
function startProfile(nonce: string, kind: ProfileKind): Promise<void> {
  return startLocalProfile(nonce, async () => {
    const id = await openWorkspaceProfile(nonce, kind, (nonce, kind) =>
      invoke("workspace_session_open", { nonce, selection: { kind } }),
    );
    sessionOrigins.set(id, { type: "profile", kind });
    return id;
  });
}
function startCustomProfile(nonce: string, id: string): Promise<void> {
  return startLocalProfile(nonce, async () => {
    const sid = await openCustomProfile(nonce, id, (nonce, id) =>
      invoke("workspace_custom_profile_open", { nonce, id }),
    );
    sessionOrigins.set(sid, { type: "custom", id });
    return sid;
  });
}
async function startLocalProfile(
  nonce: string,
  open: () => Promise<string>,
): Promise<void> {
  if (busy || fitBusy || life.signal.aborted) return;
  busy = true;
  controls();
  uiText(state, "正在启动本地 Profile…");
  try {
    const id = await open();
    if (life.signal.aborted) return;
    await listSessions(nonce);
    await attach(nonce, id);
  } catch (error) {
    if (!life.signal.aborted) {
      uiText(
        state,
        workspaceTerminalFailure(
          error,
          "启动或显示未确认 · 未重试，请刷新检查保留的会话",
        ),
      );
      try {
        await listSessions(nonce);
      } catch {
        /* A failed list never starts a replacement shell. */
      }
    }
  } finally {
    busy = false;
    controls();
  }
}
function confirmPage(): void {
  const nonce = window.__PULSE_WORKSPACE_NONCE__;
  if (!nonce || started || life.signal.aborted) return;
  started = true;
  void (async () => {
    const result = await confirmWorkspaceDocument(
      nonce,
      (value) => invoke("workspace_confirm", { nonce: value }),
      life.signal,
    );
    if (result === "cancelled") return;
    if (result !== "ready") {
      uiText(state, "页面验证未完成 · 请刷新工作区重试");
      return;
    }
    try {
      await prepareWorkspaceVisibility(document, nonce, life.signal);
    } catch {
      if (!life.signal.aborted)
        uiText(state, "窗口可见状态读取失败 · 请关闭并重新打开窗口");
      return;
    }
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    try {
      acceptSettings(await readWorkspaceSettings(nonce, life.signal));
    } catch {
      if (!life.signal.aborted)
        uiText(
          state,
          "设置读取失败 · 未使用默认值替代配置，请检查配置后重新加载",
        );
      return;
    }
    if (life.signal.aborted) return;
    await loadCommandCatalogue(nonce);
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    await loadKeymap(nonce);
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    uiText(state, "正在读取本地 Profile…");
    const profiles = await loadWorkspaceProfiles(
      nonce,
      (value) => invoke("workspace_profiles", { nonce: value }),
      life.signal,
    );
    if (profiles.status === "cancelled") return;
    if (profiles.status !== "ready") {
      uiText(state, "无法读取本地 Profile · 未使用默认 Shell");
      return;
    }
    try {
      const organization = await requestOrganization(nonce, life.signal);
      if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
        return;
      hostTree?.update(organization);
      await refreshRecentHosts(nonce);
    } catch {
      if (life.signal.aborted) return;
      hostTree?.failed();
    }
    profileManager.update(profiles.profiles);
    showWorkspaceProfiles(profileList, profiles.profiles, (kind) => {
      void startProfile(nonce, kind);
    });
    profileList.dataset.state = "ready";
    await refreshCustomProfiles(nonce);
    profileList.querySelector<HTMLButtonElement>("button")?.focus();
    uiText(
      state,
      profiles.profiles.some((p) => !p.hidden)
        ? "选择本地 Profile 创建终端"
        : "没有可显示的本地 Profile",
    );
    try {
      const ids = await listSessions(nonce);
      const selected = preferredWorkspaceSession(ids, layoutStorage());
      const preference = loadPanePreference(ids, layoutStorage());
      if (preference?.tree.type === "split")
        await showSplit(preference.tree.dir, undefined, undefined, preference);
      else if (selected) await selectSession(nonce, selected);
      const presentation = await reportWorkspacePresentation(
        nonce,
        (value) => invoke("workspace_cli_ready", { nonce: value }),
        life.signal,
        () => window.__PULSE_WORKSPACE_NONCE__,
      );
      if (presentation === "denied")
        uiText(state, "外部 CLI 就绪未确认 · 可重新加载工作区");
    } catch {
      if (!life.signal.aborted)
        uiText(state, "无法读取保留的会话 · 未启动替代 Shell");
    }
  })();
}
window.addEventListener("pulseterm:workspace-ready", confirmPage, {
  signal: life.signal,
});
function reloadWorkspace(): void {
  retireBeforeReload(
    life,
    () => {
      view?.dispose();
    },
    () => window.location.reload(),
  );
}
let commandActions: ReadonlySet<WorkspaceCommandId> | undefined;
async function loadCommandCatalogue(nonce: string): Promise<void> {
  commandActions = undefined;
  const button = element("workspace-command-open") as HTMLButtonElement;
  button.disabled = true;
  try {
    const actions = await readWorkspaceCapabilities(
      nonce,
      life.signal,
      () => window.__PULSE_WORKSPACE_NONCE__,
    );
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    commandActions = new Set(actions.map((action) => action.id));
    button.disabled = false;
    button.removeAttribute("title");
  } catch {
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    uiAttribute(button, "title", "命令目录读取失败，请重新加载");
    uiText(
      element("workspace-command-status"),
      "无法读取命令目录 · 原有按钮仍使用各自的权限检查",
    );
  }
}

const commandDialog = element("workspace-command-dialog") as HTMLDialogElement;
const historyEntry = createWorkspaceHistoryEntry({
  document,
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  fallbackOpener: element("workspace-history"),
  available: () =>
    !busy && !fitBusy && historyViewAvailable(terminal.dataset.state),
  target: () => {
    const session =
      view instanceof WorkspaceSplit ? view.focusedSession : active;
    return view && session
      ? {
          view,
          session,
          layout:
            view instanceof WorkspaceSplit ? view.layoutIdentity : undefined,
        }
      : undefined;
  },
});
const commands: readonly WorkspaceCommand[] = (
  [
    [
      "profiles",
      "管理本地 Profile",
      "profile shell 本地",
      "workspace-profile-manage",
    ],
    [
      "settings",
      "终端设置",
      "settings font cursor theme language 字体 主题 语言",
      "workspace-settings-open",
    ],
    [
      "keybindings",
      "快捷键设置",
      "keyboard shortcuts hotkeys keymap 键盘 快捷键",
      "workspace-settings-open",
    ],
    [
      "find",
      "查找当前终端",
      "find search 查找 历史 正则 整词",
      "workspace-history",
    ],
    [
      "hosts",
      "管理 SSH 主机",
      "host fingerprint known hosts 主机 指纹",
      "workspace-host-open",
    ],
    [
      "identities",
      "管理 SSH 身份",
      "identity password 用户名 密码",
      "workspace-identity-open",
    ],
    [
      "private-keys",
      "管理 SSH 私钥",
      "private key import 私钥 导入",
      "workspace-key-identity-open",
    ],
    [
      "agent-identities",
      "管理 SSH Agent 身份",
      "agent public key fingerprint 公钥 身份",
      "workspace-agent-open",
    ],
    [
      "organization",
      "分组、标签与收藏",
      "organization groups tags favorites 分组 标签 收藏",
      "workspace-organization-open",
    ],
    ["vault", "保险库", "vault lock unlock 凭据", "workspace-vault-open"],
    ["ssh", "连接保存的 SSH 主机", "ssh connect 连接", "workspace-ssh-open"],
    [
      "quick-connect",
      "快速连接",
      "quick connect address 快速连接 地址",
      "workspace-quick-connect-open",
    ],
    [
      "ssh-config",
      "导入 SSH 配置",
      "ssh config import alias 导入 配置 别名",
      "workspace-config-open",
    ],
    [
      "split-row",
      "左右分屏",
      "split horizontal left right panes 左右 分屏",
      "workspace-split-row",
    ],
    [
      "split-column",
      "上下分屏",
      "split vertical top bottom panes 上下 分屏",
      "workspace-split-column",
    ],
    [
      "duplicate-session",
      "复制当前会话",
      "duplicate clone session 复制会话 克隆",
      "workspace-duplicate",
    ],
    ["close-session", "关闭当前会话", "close session 关闭", "workspace-close"],
  ] as const
).map(([id, title, keywords, target]) => {
  const button = element(target) as HTMLButtonElement;
  return {
    id,
    title,
    keywords,
    available: () =>
      commandActions?.has(id) === true &&
      button.isConnected &&
      !button.disabled &&
      !button.hidden,
    target: () =>
      id === "find"
        ? historyEntry.target()
        : id === "close-session"
          ? ((view instanceof WorkspaceSplit ? view.focusedSession : active) ??
            "")
          : id === "split-row" || id === "split-column"
            ? JSON.stringify([
                view instanceof WorkspaceSplit ? view.focusedSession : active,
                (element("workspace-split-target") as HTMLSelectElement).value,
              ])
            : "",
    run: () => {
      if (id === "keybindings") {
        void settingsEditor.open().then(() => openKeymapSettings());
      } else button.click();
    },
  };
});
commandPalette = new WorkspaceCommandPalette({
  dialog: commandDialog,
  search: element("workspace-command-search") as HTMLInputElement,
  list: element("workspace-command-list"),
  status: element("workspace-command-status"),
  done: element("workspace-command-done") as HTMLButtonElement,
  context: {
    signal: life.signal,
    nonce: () => window.__PULSE_WORKSPACE_NONCE__,
    available: () =>
      commandActions !== undefined &&
      !busy &&
      !fitBusy &&
      profileList.dataset.state === "ready" &&
      !Array.from(document.querySelectorAll("dialog[open]")).some(
        (dialog) => dialog !== commandDialog,
      ),
  },
  commands,
});
element("workspace-command-open").addEventListener(
  "click",
  () => commandPalette?.open(),
  { signal: life.signal },
);
function keymapAvailable(): boolean {
  return (
    !life.signal.aborted &&
    !busy &&
    !fitBusy &&
    profileList.dataset.state === "ready" &&
    !document.querySelector("dialog[open]")
  );
}
function invalidateKeymap(): void {
  // Empty admission, but preserve the bounded old-completion guard across source replacement.
  keymapRuntime?.replace([]);
  keymapSource = undefined;
  keymapNonce = undefined;
}
function installKeymap(value: WorkspaceKeymap): void {
  const nonce = window.__PULSE_WORKSPACE_NONCE__;
  if (!nonce || life.signal.aborted || !value.confirmed) return;
  invalidateKeymap();
  keymapSource = value;
  keymapNonce = nonce;
  const definitions = value.values.bindings.map((binding) => ({
    action: binding.action,
    keys: formatChordSequence(binding.sequence),
    when: binding.when
      .map((term) => `${term.required ? "" : "!"}${term.field}`)
      .join(" && "),
  }));
  if (keymapRuntime) {
    keymapRuntime.replace(definitions);
    return;
  }
  keymapRuntime = new KeybindingDispatcher(definitions, {
    signal: life.signal,
    nonce: () =>
      keymapNonce === window.__PULSE_WORKSPACE_NONCE__
        ? keymapNonce
        : undefined,
    focus: () => {
      if (view && !viewIdentities.has(view))
        viewIdentities.set(view, ++nextViewIdentity);
      return `${view ? viewIdentities.get(view) : "workspace"}:${view instanceof WorkspaceSplit ? view.focusedSession : (active ?? "workspace")}`;
    },
    context: () => ({
      workspaceFocus: document.hasFocus(),
      ...classifyKeymapFocus(document.activeElement),
      modalOpen: !!document.querySelector("dialog[open]"),
      broadcast: false,
    }),
    available: (action) =>
      keymapAvailable() &&
      !classifyKeymapFocus(document.activeElement).editableFocus &&
      (action === "palette" ||
        commands.some(
          (command) => command.id === action && command.available(),
        )),
    execute: (action) => {
      if (!keymapAvailable()) return;
      if (action === "palette") {
        commandPalette?.open();
        return;
      }
      const capture = captureWorkspaceCommands(
        {
          signal: life.signal,
          nonce: () => window.__PULSE_WORKSPACE_NONCE__,
          available: keymapAvailable,
        },
        commands,
      );
      try {
        capture?.execute(action);
      } finally {
        capture?.close();
      }
    },
  });
}
async function loadKeymap(nonce: string): Promise<void> {
  invalidateKeymap();
  try {
    const candidate = await readWorkspaceKeymap(nonce, life.signal);
    if (life.signal.aborted || window.__PULSE_WORKSPACE_NONCE__ !== nonce)
      return;
    const confirmed = await confirmWorkspaceKeymap(
      nonce,
      candidate,
      life.signal,
    );
    if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
      installKeymap(confirmed);
  } catch {
    if (!life.signal.aborted && window.__PULSE_WORKSPACE_NONCE__ === nonce)
      uiText(
        element("workspace-keymap-status"),
        "键位来源不可用 · 未替换为默认键位。",
      );
  }
}
const keymapDialog = element("workspace-keymap-dialog") as HTMLDialogElement;
const keymapEditor = new WorkspaceKeymapEditor({
  dialog: keymapDialog,
  fields: element("workspace-keymap-fields"),
  status: element("workspace-keymap-status"),
  signal: life.signal,
  nonce: () => window.__PULSE_WORKSPACE_NONCE__,
  working: (value) => {
    busy = value;
    controls();
  },
  invalidate: invalidateKeymap,
  changed: installKeymap,
});
const openKeymapSettings = settingsKeymapLink({
  settings: settingsEditor,
  keymap: keymapEditor,
  dialog: keymapDialog,
  signal: life.signal,
  failed: () => {
    uiText(element("workspace-settings-status"), "无法打开快捷键设置 · 请重试");
  },
});
element("workspace-keymap-done").addEventListener(
  "click",
  () => keymapEditor.close(),
  { signal: life.signal },
);

// Context equality at the second step cannot detect focus A→B→A transitions.
window.addEventListener("focusin", () => keymapRuntime?.cancelPrefix(), {
  capture: true,
  signal: life.signal,
});
window.addEventListener("blur", () => keymapRuntime?.cancelPrefix(), {
  signal: life.signal,
});
const composition = new CompositionGuard(window, life.signal, () =>
  keymapRuntime?.cancelPrefix(),
);
const modalEscape = new WorkspaceModalEscape(life.signal, () =>
  focusedWorkspaceModal(document),
);
window.addEventListener(
  "keyup",
  (event) => {
    if (!modalEscape.keyup(event)) keymapRuntime?.keyup(event);
  },
  {
    capture: true,
    signal: life.signal,
  },
);
window.addEventListener(
  "keydown",
  (event) => {
    if (composition.handles(event)) {
      keymapRuntime?.cancelPrefix();
      return;
    }
    if (modalEscape.keydown(event)) return;
    if (keymapRuntime?.keydown(event)) return;
    // Keep the previously supported simultaneous Ctrl+Meta default palette gesture.
    // Custom/disabled palette sources never fall through to legacy shortcuts.
    if (
      keymapNonce === window.__PULSE_WORKSPACE_NONCE__ &&
      keymapSource?.values.sources.some(
        (source) =>
          source.action === "palette" &&
          source.source === "default" &&
          !source.disabled,
      ) &&
      event.ctrlKey &&
      event.metaKey &&
      commandPalette?.shortcut(event)
    )
      return;
    const direction = workspacePaneDirection(event);
    if (
      direction &&
      view instanceof WorkspaceSplit &&
      !document.querySelector("dialog[open]")
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      if (!busy && !fitBusy) view.focusDirection(direction);
      return;
    }
    if (workspaceReloadKey(event)) {
      event.preventDefault();
      event.stopImmediatePropagation();
      reloadWorkspace();
    }
  },
  { capture: true, signal: life.signal },
);
element("workspace-vault-open").addEventListener(
  "click",
  () => {
    if (!busy && !fitBusy && !document.querySelector("dialog[open]"))
      void vaultDialog.open();
  },
  { signal: life.signal },
);
element("workspace-vault-done").addEventListener(
  "click",
  () => vaultDialog.close(),
  { signal: life.signal },
);
element("workspace-ssh-open").addEventListener(
  "click",
  () => {
    if (
      window.__PULSE_WORKSPACE_SSH_CONNECTION__ === true &&
      !busy &&
      !fitBusy &&
      !document.querySelector("dialog[open]")
    )
      void sshDialog.open();
  },
  { signal: life.signal },
);
element("workspace-identity-open").addEventListener(
  "click",
  () => {
    if (busy || fitBusy || document.querySelector("dialog[open]")) return;
    uiText(element("workspace-identity-title"), "SSH 身份");
    uiText(
      element("workspace-identity-description"),
      "管理名称和用户名。解锁凭据库后可单独保存密码，不显示已保存的密码，也不授予连接权限。",
    );
    void identityEditor.open();
  },
  { signal: life.signal },
);
element("workspace-key-identity-open").addEventListener(
  "click",
  () => {
    if (busy || fitBusy || document.querySelector("dialog[open]")) return;
    uiText(element("workspace-identity-title"), "SSH 私钥身份");
    uiText(
      element("workspace-identity-description"),
      "管理私钥身份的名称与用户名。",
    );
    void keyIdentityEditor.open();
  },
  { signal: life.signal },
);
element("workspace-identity-done").addEventListener(
  "click",
  () => {
    identityEditor.close();
    keyIdentityEditor.close();
  },
  { signal: life.signal },
);
element("workspace-organization-open").addEventListener(
  "click",
  () => {
    if (!busy && !fitBusy && !document.querySelector("dialog[open]"))
      void organizationEditor.open();
  },
  { signal: life.signal },
);
element("workspace-organization-done").addEventListener(
  "click",
  () => organizationEditor.close(),
  { signal: life.signal },
);
element("workspace-host-open").addEventListener(
  "click",
  () => {
    if (!busy && !fitBusy && !document.querySelector("dialog[open]"))
      void hostEditor.open();
  },
  { signal: life.signal },
);
element("workspace-host-done").addEventListener(
  "click",
  () => hostEditor.close(),
  { signal: life.signal },
);
element("workspace-config-open").addEventListener(
  "click",
  () => {
    if (busy || fitBusy || window.__PULSE_WORKSPACE_SSH_FINGERPRINT__ !== true)
      return;
    hostEditor.close();
    configDialog.open();
  },
  { signal: life.signal },
);
element("workspace-config-done").addEventListener(
  "click",
  () => configDialog.close(),
  { signal: life.signal },
);
element("workspace-settings-open").addEventListener(
  "click",
  () => {
    if (!busy && !fitBusy && !document.querySelector("dialog[open]"))
      void settingsEditor.open();
  },
  { signal: life.signal },
);
element("workspace-settings-done").addEventListener(
  "click",
  () => settingsEditor.close(),
  { signal: life.signal },
);
element("workspace-profile-manage").addEventListener(
  "click",
  () => profileManager.open(),
  { signal: life.signal },
);
element("workspace-profile-done").addEventListener(
  "click",
  () => profileManager.close(),
  { signal: life.signal },
);
element("workspace-rename").addEventListener(
  "click",
  () => {
    if (
      !active ||
      busy ||
      fitBusy ||
      life.signal.aborted ||
      document.querySelector("dialog[open]")
    )
      return;
    editTarget = active;
    const value = labels.get(active);
    labelTitle.value = value?.title ?? "";
    labelColor.value = value?.color ?? "none";
    try {
      labelDialog.showModal();
      labelTitle.focus();
      labelTitle.select();
    } catch {
      stopLabelEditor();
      uiText(state, "标签编辑不可用");
    }
  },
  { signal: life.signal },
);
element("workspace-label-cancel").addEventListener("click", stopLabelEditor, {
  signal: life.signal,
});
labelDialog.addEventListener(
  "cancel",
  (event) => {
    event.preventDefault();
    stopLabelEditor();
  },
  { signal: life.signal },
);
element("workspace-label-form").addEventListener(
  "submit",
  (event) => {
    event.preventDefault();
    const id = editTarget;
    if (
      !id ||
      id !== active ||
      !catalog.includes(id) ||
      busy ||
      fitBusy ||
      life.signal.aborted
    ) {
      stopLabelEditor();
      return;
    }
    try {
      const value = tabLabel(labelTitle.value, labelColor.value);
      labels.set(id, value);
      const saved = saveTabLabels(catalog, labels, layoutStorage());
      stopLabelEditor();
      controls();
      uiText(
        state,
        saved
          ? "标签显示偏好已保存 · 仅此窗口"
          : "标签已更新 · 存储不可用，仅本页有效",
      );
    } catch {
      uiText(state, "标签名称最多80字符，不得包含控制或隐形方向字符");
      labelTitle.focus();
    }
  },
  { signal: life.signal },
);
element("workspace-split-ratio").addEventListener(
  "input",
  () => {
    const current = view;
    if (
      !(current instanceof WorkspaceSplit) ||
      busy ||
      fitBusy ||
      life.signal.aborted
    )
      return;
    const percent = Number(
      (element("workspace-split-ratio") as HTMLInputElement).value,
    );
    void current
      .setRatio(percent)
      .catch(() => {
        if (view === current && !life.signal.aborted)
          uiText(state, "比例调整未确认 · 请等待窗格恢复");
      })
      .finally(() => {
        if (view === current && !life.signal.aborted) controls();
      });
  },
  { signal: life.signal },
);
element("workspace-pane-maximize").addEventListener(
  "click",
  () => {
    const current = view;
    if (
      !(current instanceof WorkspaceSplit) ||
      busy ||
      fitBusy ||
      !current.canChangeLayout
    )
      return;
    void current.toggleMaximized().catch(() => {
      if (view === current && !life.signal.aborted)
        uiText(state, "显示调整未确认 · 请等待窗格恢复");
    });
  },
  { signal: life.signal },
);
element("workspace-pane-single").addEventListener(
  "click",
  () => {
    const current = view,
      nonce = window.__PULSE_WORKSPACE_NONCE__;
    if (!(current instanceof WorkspaceSplit) || !nonce || busy || fitBusy)
      return;
    busy = true;
    controls();
    void attach(nonce, current.focusedSession)
      .catch((error: unknown) => {
        if (!life.signal.aborted)
          uiText(
            state,
            workspaceTerminalFailure(error, "显示未确认 · 原会话仍保留"),
          );
      })
      .finally(() => {
        busy = false;
        controls();
      });
  },
  { signal: life.signal },
);
element("workspace-split-row").addEventListener(
  "click",
  () => {
    void showSplit("row");
  },
  { signal: life.signal },
);
element("workspace-split-column").addEventListener(
  "click",
  () => {
    void showSplit("column");
  },
  { signal: life.signal },
);
element("workspace-reload").addEventListener("click", reloadWorkspace, {
  signal: life.signal,
});
element("workspace-history").addEventListener(
  "click",
  () => historyEntry.open(),
  { signal: life.signal },
);
duplicate.addEventListener(
  "click",
  () => {
    const nonce = window.__PULSE_WORKSPACE_NONCE__,
      id = view instanceof WorkspaceSplit ? view.focusedSession : active;
    if (!nonce || !id || busy || fitBusy || life.signal.aborted) return;
    const origin = sessionOrigins.get(id);
    if (origin?.type === "profile") {
      void startProfile(nonce, origin.kind);
    } else if (origin?.type === "custom") {
      void startCustomProfile(nonce, origin.id);
    } else {
      uiText(
        state,
        "当前页面尚未记录该会话来源，请从主机或 Profile 重新连接后再复制",
      );
    }
  },
  { signal: life.signal },
);
close.addEventListener(
  "click",
  () => {
    const nonce = window.__PULSE_WORKSPACE_NONCE__,
      id = view instanceof WorkspaceSplit ? view.focusedSession : active;
    if (!nonce || !id || busy || fitBusy || life.signal.aborted) return;
    busy = true;
    controls();
    void closeWorkspaceSession(
      id,
      life.signal,
      () =>
        (view instanceof WorkspaceSplit ? view.focusedSession : active) ===
          id && window.__PULSE_WORKSPACE_NONCE__ === nonce,
      () =>
        display?.confirmClose === false
          ? Promise.resolve(true)
          : confirmWorkspaceClose(terminal.ownerDocument, id, life.signal),
      async () => {
        view?.dispose();
        view = undefined;
        uiText(state, "正在关闭并回收会话…");
        await invoke("workspace_session_close", { nonce, session: id });
        sessionOrigins.delete(id);
      },
    )
      .then(async (result) => {
        if (life.signal.aborted) return;
        if (result === "cancelled") {
          uiText(state, "已保留会话");
          return;
        }
        active = undefined;
        terminal.replaceChildren();
        terminal.dataset.state = "empty";
        uiText(caption, "尚无会话");
        uiText(state, "会话已关闭");
        const ids = await listSessions(nonce);
        const preference = loadPanePreference(ids, layoutStorage());
        busy = false;
        if (preference?.tree.type === "split")
          await showSplit(
            preference.tree.dir,
            undefined,
            undefined,
            preference,
          );
        else if (preference) await attach(nonce, preference.focused);
        else if (ids[0]) await attach(nonce, ids[0]);
      })
      .catch(() => {
        if (!life.signal.aborted)
          uiText(state, "关闭结果未确认 · 请刷新检查，不自动重试");
      })
      .finally(() => {
        busy = false;
        controls();
      });
  },
  { signal: life.signal },
);
window.addEventListener(
  "keydown",
  (event) => {
    if (composition.handles(event)) return;
    if (
      (event.ctrlKey || event.metaKey) &&
      event.shiftKey &&
      event.key.toLowerCase() === "r"
    ) {
      event.preventDefault();
      event.stopImmediatePropagation();
      window.location.reload();
    }
  },
  { signal: life.signal, capture: true },
);
window.addEventListener(
  "pagehide",
  () => {
    life.abort();

    view?.dispose();
  },
  { once: true },
);
window.addEventListener(
  "beforeunload",
  (event) => {
    if (display?.confirmClose !== false && catalog.length > 0) {
      event.preventDefault();
      event.returnValue = "";
    }
  },
  { signal: life.signal },
);
confirmPage();
