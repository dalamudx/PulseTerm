import {
  decodeWorkspaceProfiles,
  type ProfileKind,
  type WorkspaceProfile,
} from "./workspace-profiles";
import { isWorkspaceNonce } from "./workspace-readiness";
export async function changeProfileVisibility(
  nonce: string,
  kind: ProfileKind,
  hidden: boolean,
  offered: readonly WorkspaceProfile[],
  signal: AbortSignal,
  send: (nonce: string, kind: ProfileKind, hidden: boolean) => Promise<unknown>,
): Promise<"saved" | "cancelled" | "uncertain"> {
  if (signal.aborted) return "cancelled";
  if (
    !isWorkspaceNonce(nonce) ||
    typeof hidden !== "boolean" ||
    !decodeWorkspaceProfiles(offered).some((p) => p.kind === kind)
  )
    throw Error("workspace.denied");
  let cancel: (() => void) | undefined,
    timer: ReturnType<typeof setTimeout> | undefined;
  const stopped = Symbol("stopped");
  const interruption = new Promise<symbol>((resolve) => {
    cancel = () => resolve(stopped);
    signal.addEventListener("abort", cancel, { once: true });
    timer = setTimeout(() => resolve(stopped), 5000);
  });
  try {
    const result = await Promise.race([
      send(nonce, kind, hidden),
      interruption,
    ]);
    if (signal.aborted) return "cancelled";
    if (result === stopped) return "uncertain";
    if (result !== null && result !== undefined)
      throw Error("workspace.visibility_receipt");
    return "saved";
  } catch {
    return signal.aborted ? "cancelled" : "uncertain";
  } finally {
    clearTimeout(timer);
    if (cancel) signal.removeEventListener("abort", cancel);
  }
}
