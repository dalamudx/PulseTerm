import { isWorkspaceNonce } from "./workspace-readiness";

export const PROFILE_KINDS = [
  "bash",
  "zsh",
  "fish",
  "sh",
  "pwsh",
  "powershell",
  "cmd",
  "wsl",
] as const;
export type ProfileKind = (typeof PROFILE_KINDS)[number];
export interface WorkspaceProfile {
  readonly kind: ProfileKind;
  readonly hidden: boolean;
}
const KINDS = new Set<string>(PROFILE_KINDS);
export function isProfileKind(value: unknown): value is ProfileKind {
  return typeof value === "string" && KINDS.has(value);
}
const LABELS: Record<ProfileKind, string> = {
  bash: "Bash",
  zsh: "Zsh",
  fish: "Fish",
  sh: "POSIX sh",
  pwsh: "PowerShell (pwsh)",
  powershell: "Windows PowerShell",
  cmd: "Command Prompt",
  wsl: "WSL (default distribution)",
};

/** Path-free snapshot only; never a launch receipt. Malformed native results are not empty catalogs. */
export function decodeWorkspaceProfiles(
  input: unknown,
): readonly WorkspaceProfile[] {
  if (!Array.isArray(input) || input.length > PROFILE_KINDS.length)
    throw new Error("workspace.profiles_invalid");
  const seen = new Set<string>();
  return Array.from(input, (value: unknown) => {
    if (typeof value !== "object" || value === null || Array.isArray(value))
      throw new Error("workspace.profiles_invalid");
    const item = value as Record<string, unknown>;
    if (
      Object.keys(item).length !== 2 ||
      !Object.hasOwn(item, "kind") ||
      !Object.hasOwn(item, "hidden") ||
      typeof item.kind !== "string" ||
      !KINDS.has(item.kind) ||
      typeof item.hidden !== "boolean" ||
      seen.has(item.kind)
    )
      throw new Error("workspace.profiles_invalid");
    seen.add(item.kind);
    return Object.freeze({
      kind: item.kind as ProfileKind,
      hidden: item.hidden,
    });
  });
}
export type ProfileResult =
  | { status: "ready"; profiles: readonly WorkspaceProfile[] }
  | { status: "cancelled" | "unavailable" };
export async function loadWorkspaceProfiles(
  nonce: string,
  send: (nonce: string) => Promise<unknown>,
  signal: AbortSignal,
): Promise<ProfileResult> {
  if (signal.aborted) return { status: "cancelled" };
  if (!isWorkspaceNonce(nonce)) return { status: "unavailable" };
  const stopped = Symbol("stopped");
  let cancel: (() => void) | undefined;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const interruption = new Promise<symbol>((resolve) => {
    cancel = () => resolve(stopped);
    signal.addEventListener("abort", cancel, { once: true });
    timer = setTimeout(() => resolve(stopped), 5_000);
  });
  try {
    const input = await Promise.race([send(nonce), interruption]);
    if (signal.aborted) return { status: "cancelled" };
    if (input === stopped) return { status: "unavailable" };
    return { status: "ready", profiles: decodeWorkspaceProfiles(input) };
  } catch {
    return { status: signal.aborted ? "cancelled" : "unavailable" };
  } finally {
    clearTimeout(timer);
    if (cancel) signal.removeEventListener("abort", cancel);
  }
}
export function showWorkspaceProfiles(
  list: HTMLElement,
  profiles: readonly WorkspaceProfile[],
  select?: (kind: ProfileKind) => void,
): void {
  list.replaceChildren();
  for (const profile of profiles) {
    if (profile.hidden) continue;
    const row = list.ownerDocument.createElement("li");
    if (select) {
      const button = list.ownerDocument.createElement("button");
      button.type = "button";
      button.textContent = LABELS[profile.kind];
      button.dataset.profile = profile.kind;
      button.addEventListener("click", () => select(profile.kind));
      row.append(button);
    } else row.textContent = LABELS[profile.kind];
    list.append(row);
  }
}
