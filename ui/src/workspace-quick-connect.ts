import {
  type Host,
  type HostFields,
  validateHostChange,
} from "./workspace-hosts";

const hidden =
  /[\p{Cc}\s\u00ad\u061c\u200b-\u200f\u2028-\u202e\u2060-\u206f\ufeff]/u;
function invalid(): never {
  throw Error("quick_connect.invalid");
}
function normalizedHost(raw: string): string {
  if (raw.includes(":")) {
    try {
      const host = new URL(`http://[${raw}]/`).hostname;
      if (!host.startsWith("[") || !host.endsWith("]")) return invalid();
      return host.slice(1, -1);
    } catch {
      return invalid();
    }
  }
  if (
    raw.length > 253 ||
    raw
      .split(".")
      .some(
        (label) =>
          !label ||
          label.length > 63 ||
          !/^[a-zA-Z0-9](?:[a-zA-Z0-9-]*[a-zA-Z0-9])?$/.test(label),
      )
  )
    return invalid();
  return raw.toLowerCase();
}
/** A metadata preview only; the existing backend save validates every field again. */
export function parseQuickTarget(address: string, label?: string): HostFields {
  if (!address || address.length > 1024 || hidden.test(address))
    return invalid();
  const raw = address.startsWith("ssh://") ? address.slice(6) : address;
  if (/[/?#%\\]/.test(raw)) return invalid();
  const at = raw.lastIndexOf("@");
  if (at <= 0 || at === raw.length - 1) return invalid();
  const username = raw.slice(0, at),
    authority = raw.slice(at + 1);
  if (username.includes(":") || username.startsWith("-")) return invalid();
  let host: string,
    port = 22;
  let portText: string | undefined;
  if (authority.startsWith("[")) {
    const end = authority.indexOf("]");
    if (end < 2) return invalid();
    host = authority.slice(1, end);
    if (!host.includes(":")) return invalid();
    const suffix = authority.slice(end + 1);
    if (suffix) {
      if (!suffix.startsWith(":")) return invalid();
      portText = suffix.slice(1);
    }
  } else {
    const parts = authority.split(":");
    if (parts.length > 2) return invalid();
    host = parts[0] ?? "";
    portText = parts[1];
  }
  if (portText !== undefined) {
    if (!/^[1-9][0-9]{0,4}$/.test(portText)) return invalid();
    port = Number(portText);
  }
  host = normalizedHost(host);
  const fields = { label: label || host, host, port, username };
  validateHostChange({ action: "create", ...fields });
  return Object.freeze(fields);
}
export function matchingQuickHosts(
  hosts: readonly Host[],
  target: HostFields,
): readonly Host[] {
  return hosts.filter(
    (host) =>
      normalizedHost(host.host) === target.host &&
      host.port === target.port &&
      host.username === target.username,
  );
}
