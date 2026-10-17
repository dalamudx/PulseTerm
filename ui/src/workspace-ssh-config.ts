import { invoke } from "@tauri-apps/api/core";
export type ConfigPreview = Readonly<{
  version: 1;
  token: string;
  host: string;
  port: number;
  username: string;
  identityFileCount: number;
}>;
export type ConfigRpc = (
  command: string,
  args: Record<string, unknown>,
) => Promise<unknown>;
const uuid =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/;
const hidden =
  /[\p{Cc}\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u;
function bad(): never {
  throw Error("config.invalid");
}
export function validateConfigAlias(value: unknown): string {
  if (
    typeof value !== "string" ||
    value.length === 0 ||
    value.length > 253 ||
    !/^[a-zA-Z0-9._:-]+$/.test(value)
  )
    return bad();
  return value;
}
export function decodeConfigPreview(raw: unknown): ConfigPreview {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return bad();
  const r = raw as Record<string, unknown>;
  const keys = [
    "version",
    "token",
    "host",
    "port",
    "username",
    "identityFileCount",
  ];
  if (
    Object.keys(r).length !== keys.length ||
    keys.some((k) => !Object.hasOwn(r, k)) ||
    r.version !== 1 ||
    typeof r.token !== "string" ||
    !uuid.test(r.token) ||
    typeof r.host !== "string" ||
    !r.host.length ||
    r.host.length > 253 ||
    !/^[a-zA-Z0-9.:-]+$/.test(r.host) ||
    r.host !== r.host.toLowerCase() ||
    typeof r.username !== "string" ||
    !r.username.length ||
    new TextEncoder().encode(r.username).length > 255 ||
    hidden.test(r.username) ||
    /\s/u.test(r.username) ||
    r.username.startsWith("-") ||
    typeof r.port !== "number" ||
    !Number.isInteger(r.port) ||
    r.port < 1 ||
    r.port > 65535 ||
    typeof r.identityFileCount !== "number" ||
    !Number.isInteger(r.identityFileCount) ||
    r.identityFileCount < 0 ||
    r.identityFileCount > 32
  )
    return bad();
  return Object.freeze({
    version: 1,
    token: r.token,
    host: r.host,
    port: r.port,
    username: r.username,
    identityFileCount: r.identityFileCount,
  });
}
async function request(
  command: string,
  args: Record<string, unknown>,
  signal: AbortSignal,
  rpc: ConfigRpc,
): Promise<unknown> {
  if (signal.aborted) throw Error("config.cancelled");
  let timer: ReturnType<typeof setTimeout> | undefined;
  let abort = () => {};
  const stopped = new Promise<never>((_, reject) => {
    abort = () => reject(Error("config.cancelled"));
    signal.addEventListener("abort", abort, { once: true });
    timer = setTimeout(() => reject(Error("config.timeout")), 5000);
  });
  try {
    const raw = await Promise.race([rpc(command, args), stopped]);
    if (signal.aborted) throw Error("config.cancelled");
    return raw;
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", abort);
  }
}
export async function readConfigPreview(
  nonce: string,
  alias: string,
  signal: AbortSignal,
  rpc: ConfigRpc = invoke,
): Promise<ConfigPreview> {
  if (!uuid.test(nonce)) return bad();
  return decodeConfigPreview(
    await request(
      "workspace_ssh_config_preview",
      { nonce, alias: validateConfigAlias(alias) },
      signal,
      rpc,
    ),
  );
}
export async function confirmConfigPreview(
  nonce: string,
  token: string,
  signal: AbortSignal,
  rpc: ConfigRpc = invoke,
): Promise<void> {
  if (!uuid.test(nonce) || !uuid.test(token)) return bad();
  const result = await request(
    "workspace_ssh_config_confirm",
    { nonce, tokenValue: token },
    signal,
    rpc,
  );
  if (result !== null) return bad();
}

export async function checkConfigSource(
  nonce: string,
  token: string,
  signal: AbortSignal,
  rpc: ConfigRpc = invoke,
): Promise<"unchanged" | "changed"> {
  if (!uuid.test(nonce) || !uuid.test(token)) return bad();
  const raw = await request(
    "workspace_ssh_config_status",
    { nonce, tokenValue: token },
    signal,
    rpc,
  );
  if (
    typeof raw !== "object" ||
    raw === null ||
    Array.isArray(raw) ||
    Object.getPrototypeOf(raw) !== Object.prototype ||
    Reflect.ownKeys(raw).length !== 2 ||
    !Object.hasOwn(raw, "version") ||
    !Object.hasOwn(raw, "status")
  )
    return bad();
  const version = Object.getOwnPropertyDescriptor(raw, "version");
  const status = Object.getOwnPropertyDescriptor(raw, "status");
  if (
    !version ||
    !status ||
    !("value" in version) ||
    !("value" in status) ||
    version.value !== 1 ||
    (status.value !== "unchanged" && status.value !== "changed")
  )
    return bad();
  return status.value;
}
