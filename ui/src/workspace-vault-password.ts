import { invoke } from "@tauri-apps/api/core";
import {
  performVaultHandoff,
  type VaultTicket,
} from "./workspace-vault-handoff";
/** Owned bytes only. JS strings/IPC transport/native copies cannot be completely wiped by this helper. */
export async function submitMasterPassword(
  kind: VaultTicket["kind"],
  nonce: string,
  bytes: Uint8Array,
  signal: AbortSignal,
) {
  let wire: number[] = [];
  const wipe = () => {
    bytes.fill(0);
    wire.fill(0);
  };
  signal.addEventListener("abort", wipe, { once: true });
  try {
    if (bytes.byteLength < 1 || bytes.byteLength > 4096 || signal.aborted)
      throw Error("vault.password_invalid");
    return await performVaultHandoff(kind, nonce, signal, (ticket) => {
      if (signal.aborted) throw Error("vault.cancelled");
      wire = Array.from(bytes);
      const request = invoke<unknown>("workspace_vault_submit", {
        nonce,
        token: ticket.token,
        kind: ticket.kind,
        mode: "master_password",
        password: wire,
      });
      bytes.fill(0);
      return request;
    });
  } finally {
    wipe();
    signal.removeEventListener("abort", wipe);
  }
}
