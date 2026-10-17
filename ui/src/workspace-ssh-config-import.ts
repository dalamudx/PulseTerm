import { invoke } from "@tauri-apps/api/core";
import { type ConfigRpc, decodeConfigPreview } from "./workspace-ssh-config";
export type ConfigImportPreview = Readonly<
  ReturnType<typeof decodeConfigPreview> & { label: string }
>;
const uuid4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export function validateImportLabel(value: unknown): string {
  if (
    typeof value !== "string" ||
    !value.length ||
    value.trim() !== value ||
    new TextEncoder().encode(value).length > 256 ||
    /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u.test(
      value,
    )
  )
    throw Error("config.invalid");
  return value;
}
export function decodeConfigImport(raw: unknown): ConfigImportPreview {
  if (!raw || typeof raw !== "object" || Array.isArray(raw))
    throw Error("config.invalid");
  const { label, ...preview } = raw as Record<string, unknown>;
  return Object.freeze({
    ...decodeConfigPreview(preview),
    label: validateImportLabel(label),
  });
}
export async function cancelConfigImport(
  nonce: string,
  token: string,
  rpc: ConfigRpc = invoke,
): Promise<void> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) return;
  try {
    await rpc("workspace_ssh_config_import_cancel", {
      nonce,
      tokenValue: token,
    });
  } catch {}
}
async function request(
  nonce: string,
  token: string,
  signal: AbortSignal,
  rpc: ConfigRpc,
  prepare: boolean,
  label?: string,
): Promise<unknown> {
  if (!uuid4.test(nonce) || !uuid4.test(token)) throw Error("config.invalid");
  if (signal.aborted) throw Error("config.cancelled");
  let cancelled = false;
  let timeout: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stop = new Promise<never>((_, reject) => {
    abort = () => {
      cancelled = true;
      if (!prepare) void cancelConfigImport(nonce, token, rpc);
      reject(Error("config.cancelled"));
    };
    signal.addEventListener("abort", abort, { once: true });
    timeout = setTimeout(() => {
      cancelled = true;
      if (!prepare) void cancelConfigImport(nonce, token, rpc);
      reject(Error("config.timeout"));
    }, 5000);
  });
  const args = prepare
    ? { nonce, tokenValue: token, label: validateImportLabel(label) }
    : { nonce, tokenValue: token };
  try {
    const pending = rpc(
      prepare
        ? "workspace_ssh_config_import_prepare"
        : "workspace_ssh_config_import_save",
      args,
    ).then((raw) => {
      if (prepare && cancelled) {
        if (raw && typeof raw === "object" && !Array.isArray(raw)) {
          const value = (raw as Record<string, unknown>).token;
          if (typeof value === "string")
            void cancelConfigImport(nonce, value, rpc);
        }
        throw Error("config.cancelled");
      }
      return raw;
    });
    const raw = await Promise.race([pending, stop]);
    if (signal.aborted) {
      abort();
      throw Error("config.cancelled");
    }
    return raw;
  } finally {
    clearTimeout(timeout);
    signal.removeEventListener("abort", abort);
  }
}
export async function prepareConfigImport(
  nonce: string,
  token: string,
  label: string,
  signal: AbortSignal,
  rpc: ConfigRpc = invoke,
): Promise<ConfigImportPreview> {
  validateImportLabel(label);
  const raw = await request(nonce, token, signal, rpc, true, label);
  try {
    return decodeConfigImport(raw);
  } catch (error) {
    if (raw && typeof raw === "object" && !Array.isArray(raw)) {
      const value = (raw as Record<string, unknown>).token;
      if (typeof value === "string") void cancelConfigImport(nonce, value, rpc);
    }
    throw error;
  }
}
export async function saveConfigImport(
  nonce: string,
  token: string,
  signal: AbortSignal,
  rpc: ConfigRpc = invoke,
): Promise<string> {
  const raw = await request(nonce, token, signal, rpc, false);
  if (typeof raw !== "string" || !uuid7.test(raw))
    throw Error("config.invalid");
  return raw;
}
