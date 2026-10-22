import { isProfileKind, type ProfileKind } from "./workspace-profiles";
import { isWorkspaceNonce } from "./workspace-readiness";
export function decodeWorkspaceSessions(value: unknown): readonly string[] {
  if (!Array.isArray(value) || value.length > 64)
    throw new Error("workspace.sessions_invalid");
  const ids = value.map((id) => {
    if (
      typeof id !== "string" ||
      !/^[1-9][0-9]{0,19}$/.test(id) ||
      BigInt(id) > (1n << 64n) - 1n
    )
      throw new Error("workspace.sessions_invalid");
    return id;
  });
  if (new Set(ids).size !== ids.length)
    throw new Error("workspace.sessions_invalid");
  return Object.freeze(ids);
}
/** One explicit request, never retry a launch with an uncertain response. */
export async function openWorkspaceProfile(
  nonce: string,
  kind: ProfileKind,
  send: (nonce: string, kind: ProfileKind) => Promise<unknown>,
): Promise<string> {
  if (!isWorkspaceNonce(nonce) || !isProfileKind(kind))
    throw new Error("workspace.denied");
  const id = decodeWorkspaceSessions([await send(nonce, kind)])[0];
  if (!id) throw new Error("workspace.sessions_invalid");
  return id;
}
