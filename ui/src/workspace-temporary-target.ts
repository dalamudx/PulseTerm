import { invoke } from "@tauri-apps/api/core";
import {
  decodeHostCatalogue,
  type Host,
  type HostFields,
  validateHostChange,
} from "./workspace-hosts";
export type TemporaryTarget = Readonly<{
  version: 1;
  token: string;
  target: Host;
}>;
function exact(
  value: unknown,
  keys: readonly string[],
): Record<string, unknown> {
  if (
    !value ||
    typeof value !== "object" ||
    Array.isArray(value) ||
    Object.keys(value).length !== keys.length ||
    !keys.every((key) => Object.hasOwn(value, key))
  )
    throw Error("quick_target.invalid");
  return value as Record<string, unknown>;
}
export function decodeTemporaryTarget(value: unknown): TemporaryTarget {
  const raw = exact(value, ["version", "token", "target"]);
  if (raw.version !== 1) throw Error("quick_target.invalid");
  const catalogue = decodeHostCatalogue({
    version: 1,
    token: raw.token,
    revision: "0",
    hosts: [raw.target],
  });
  const target = catalogue.hosts[0];
  if (!target) throw Error("quick_target.invalid");
  return { version: 1, token: catalogue.token, target };
}
export async function cancelTemporaryTarget(
  nonce: string,
  token: string,
): Promise<void> {
  try {
    await invoke("workspace_quick_target_cancel", { nonce, token });
  } catch {
    /* The old page may already be retired. */
  }
}
export async function prepareTemporaryTarget(
  nonce: string,
  fields: HostFields,
  signal: AbortSignal,
): Promise<TemporaryTarget> {
  validateHostChange({ action: "create", ...fields });
  if (signal.aborted) throw Error("quick_target.cancelled");
  const target = decodeTemporaryTarget(
    await invoke("workspace_quick_target_prepare", {
      nonce,
      value: {
        label: fields.label,
        hostname: fields.host,
        port: fields.port,
        username: fields.username,
      },
    }),
  );
  if (signal.aborted) {
    await cancelTemporaryTarget(nonce, target.token);
    throw Error("quick_target.cancelled");
  }
  return target;
}
export async function saveTemporaryTarget(
  nonce: string,
  target: TemporaryTarget,
  label: string,
  signal: AbortSignal,
): Promise<string> {
  validateHostChange({
    action: "create",
    label,
    host: target.target.host,
    port: target.target.port,
    username: target.target.username,
  });
  if (signal.aborted) throw Error("quick_target.cancelled");
  const raw = exact(
    await invoke("workspace_quick_target_save", {
      nonce,
      token: target.token,
      label,
    }),
    ["version", "host"],
  );
  if (raw.version !== 1 || signal.aborted)
    throw Error("quick_target.unconfirmed");
  const saved = decodeHostCatalogue({
    version: 1,
    token: target.token,
    revision: "0",
    hosts: [
      {
        id: raw.host,
        label: target.target.label,
        host: target.target.host,
        port: target.target.port,
        username: target.target.username,
      },
    ],
  }).hosts[0];
  if (!saved) throw Error("quick_target.invalid");
  return saved.id;
}
