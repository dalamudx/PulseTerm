import { invoke } from "@tauri-apps/api/core";
export type RecentHost = Readonly<{ host: string; connectedAt: number }>;
const uuid7 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
export function decodeRecentHosts(raw: unknown): readonly RecentHost[] {
  if (!Array.isArray(raw) || raw.length > 32) throw Error("recent.invalid");
  const ids = new Set<string>();
  return raw.map((item: unknown) => {
    if (!item || typeof item !== "object" || Array.isArray(item))
      throw Error("recent.invalid");
    const row = item as Record<string, unknown>;
    if (
      Object.keys(row).length !== 2 ||
      typeof row.host !== "string" ||
      !uuid7.test(row.host) ||
      ids.has(row.host) ||
      !Number.isSafeInteger(row.connectedAt) ||
      (row.connectedAt as number) <= 0
    )
      throw Error("recent.invalid");
    ids.add(row.host);
    return { host: row.host, connectedAt: row.connectedAt as number };
  });
}
export async function requestRecentHosts(
  nonce: string,
  signal: AbortSignal,
): Promise<readonly RecentHost[]> {
  if (signal.aborted) throw Error("recent.cancelled");
  const value = await invoke("workspace_recent_hosts", { nonce });
  if (signal.aborted) throw Error("recent.cancelled");
  return decodeRecentHosts(value);
}
