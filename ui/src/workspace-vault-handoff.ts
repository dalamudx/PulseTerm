import { invoke } from "@tauri-apps/api/core";
import { decodeVaultStatus, type VaultStatus } from "./workspace-vault";
export type VaultTicket = Readonly<{
  version: 1;
  token: string;
  kind: "create" | "unlock";
}>;
const uuid = /^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$/;
const token =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export function decodeVaultTicket(value: unknown): VaultTicket {
  if (!value || typeof value !== "object" || Array.isArray(value))
    throw Error("vault.ticket_invalid");
  const r = value as Record<string, unknown>;
  if (
    Object.keys(r).length !== 3 ||
    !Object.hasOwn(r, "version") ||
    !Object.hasOwn(r, "token") ||
    !Object.hasOwn(r, "kind") ||
    r.version !== 1 ||
    typeof r.token !== "string" ||
    !token.test(r.token) ||
    (r.kind !== "create" && r.kind !== "unlock")
  )
    throw Error("vault.ticket_invalid");
  return Object.freeze({ version: 1, token: r.token, kind: r.kind });
}
async function bounded<T>(work: Promise<T>, signal: AbortSignal): Promise<T> {
  if (signal.aborted) throw Error("vault.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  try {
    const ended = new Promise<never>((_, reject) => {
      abort = () => reject(Error("vault.cancelled"));
      signal.addEventListener("abort", abort, { once: true });
      timer = setTimeout(() => reject(Error("vault.timeout")), 5000);
    });
    const value = await Promise.race([work, ended]);
    if (signal.aborted) throw Error("vault.cancelled");
    return value;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
/** Native auth is not acceptance. Aborting after confirm dispatch is uncertain, never an automatic retry or connection grant. */
export async function performVaultHandoff(
  kind: VaultTicket["kind"],
  nonce: string,
  signal: AbortSignal,
  submit: (ticket: VaultTicket, signal: AbortSignal) => Promise<unknown>,
): Promise<VaultStatus> {
  if (!uuid.test(nonce) || nonce === "00000000-0000-0000-0000-000000000000")
    throw Error("vault.nonce");
  if (signal.aborted) throw Error("vault.cancelled");
  let issued: VaultTicket | undefined;
  let failed = false;
  let confirmSent = false;
  let cancel: Promise<void> | undefined;
  const cancelOnce = (id: string) => {
    cancel ??= invoke<void>("workspace_vault_cancel", {
      nonce,
      token: id,
    }).catch(() => {});
    return cancel;
  };
  try {
    const prepared = invoke<unknown>("workspace_vault_prepare", {
      nonce,
      kind,
    });
    void prepared.then(
      (value) => {
        if (!signal.aborted && !failed) return;
        try {
          void cancelOnce(decodeVaultTicket(value).token);
        } catch {}
      },
      () => {},
    );
    issued = decodeVaultTicket(await bounded(prepared, signal));
    if (issued.kind !== kind) throw Error("vault.ticket_invalid");
    if (signal.aborted) throw Error("vault.cancelled");
    const ready = decodeVaultStatus(
      await bounded(submit(issued, signal), signal),
    );
    if (kind === "create") {
      if (ready.state !== "locked") throw Error("vault.ready_invalid");
      return ready;
    }
    if (ready.state !== "pending") throw Error("vault.ready_invalid");
    if (signal.aborted) throw Error("vault.cancelled");
    confirmSent = true;
    const accepted = decodeVaultStatus(
      await bounded(
        invoke<unknown>("workspace_vault_confirm", {
          nonce,
          token: issued.token,
        }),
        signal,
      ),
    );
    if (accepted.state !== "available") throw Error("vault.confirm_invalid");
    return accepted;
  } catch (error) {
    failed = true;
    if (issued) void cancelOnce(issued.token);
    if (confirmSent) throw Error("vault.confirm_uncertain");
    throw error;
  }
}
