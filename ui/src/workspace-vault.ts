import { invoke } from "@tauri-apps/api/core";
export type VaultStatus = Readonly<{
  version: 1;
  state: "missing" | "locked" | "pending" | "available";
}>;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
export function decodeVaultStatus(value: unknown): VaultStatus {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("vault.invalid");
  const record = value as Record<string, unknown>;
  if (
    Object.keys(record).length !== 2 ||
    !Object.hasOwn(record, "version") ||
    !Object.hasOwn(record, "state") ||
    record.version !== 1 ||
    !["missing", "locked", "pending", "available"].includes(
      record.state as string,
    )
  )
    throw Error("vault.invalid");
  return Object.freeze({
    version: 1,
    state: record.state as VaultStatus["state"],
  });
}
async function request(
  command: "workspace_vault_status" | "workspace_vault_lock",
  nonce: string,
  signal: AbortSignal,
): Promise<VaultStatus> {
  if (!uuid.test(nonce) || nonce === "00000000-0000-0000-0000-000000000000")
    throw Error("vault.nonce");
  if (signal.aborted) throw Error("vault.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  try {
    const cancel = new Promise<never>((_, reject) => {
      abort = () => reject(Error("vault.cancelled"));
      signal.addEventListener("abort", abort, { once: true });
      timer = setTimeout(() => reject(Error("vault.timeout")), 5000);
    });
    const value = await Promise.race([
      invoke<unknown>(command, { nonce }),
      cancel,
    ]);
    if (signal.aborted) throw Error("vault.cancelled");
    return decodeVaultStatus(value);
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export const readVaultStatus = (nonce: string, signal: AbortSignal) =>
  request("workspace_vault_status", nonce, signal);
export const lockWorkspaceVault = (nonce: string, signal: AbortSignal) =>
  request("workspace_vault_lock", nonce, signal);
