import { decodeWorkspaceSessions } from "./workspace-sessions";
export const TAB_COLORS = [
  "none",
  "cyan",
  "blue",
  "green",
  "amber",
  "red",
] as const;
export type TabColor = (typeof TAB_COLORS)[number];
export type TabLabel = Readonly<{ title: string; color: TabColor }>;
const KEY = "pulseterm.workspace.labels.v1";
export function tabLabel(title: string, color: string): TabLabel {
  const clean = title.trim();
  if (
    title.length > 256 ||
    clean.length > 80 ||
    new TextEncoder().encode(clean).length > 256 ||
    ![...title].every((c) => {
      const n = c.codePointAt(0) ?? 0;
      return (
        n >= 32 &&
        !(n >= 127 && n <= 159) &&
        !(n >= 0xd800 && n <= 0xdfff) &&
        n !== 0x061c &&
        !(n >= 0x200b && n <= 0x200f) &&
        !(n >= 0x2028 && n <= 0x202e) &&
        !(n >= 0x2060 && n <= 0x206f) &&
        n !== 0xfeff
      );
    })
  )
    throw Error("workspace.label_invalid");
  if (!(TAB_COLORS as readonly string[]).includes(color))
    throw Error("workspace.color_invalid");
  return Object.freeze({ title: clean, color: color as TabColor });
}
/** Window-local display preferences only, never session ownership or launch settings. */
export function loadTabLabels(
  sessions: readonly string[],
  storage?: Pick<Storage, "getItem">,
): ReadonlyMap<string, TabLabel> {
  const allowed = new Set(decodeWorkspaceSessions(sessions)),
    out = new Map<string, TabLabel>();
  try {
    const raw = storage?.getItem(KEY);
    if (!raw) return out;
    if (raw.length > 32768) throw Error("size");
    const value = JSON.parse(raw) as unknown;
    if (!value || typeof value !== "object" || Array.isArray(value))
      throw Error("shape");
    const root = value as Record<string, unknown>;
    if (
      Object.keys(root).length !== 2 ||
      root.version !== 1 ||
      !Array.isArray(root.entries) ||
      root.entries.length > 64
    )
      throw Error("shape");
    const seen = new Set<string>();
    for (const entry of root.entries) {
      if (
        !entry ||
        typeof entry !== "object" ||
        Array.isArray(entry) ||
        Object.keys(entry).length !== 3 ||
        !Object.hasOwn(entry, "session") ||
        !Object.hasOwn(entry, "title") ||
        !Object.hasOwn(entry, "color") ||
        typeof entry.session !== "string" ||
        typeof entry.title !== "string" ||
        typeof entry.color !== "string" ||
        seen.has(entry.session)
      )
        throw Error("entry");
      decodeWorkspaceSessions([entry.session]);
      seen.add(entry.session);
      const label = tabLabel(entry.title, entry.color);
      if (allowed.has(entry.session)) out.set(entry.session, label);
    }
  } catch {
    return new Map();
  }
  return out;
}
export function saveTabLabels(
  sessions: readonly string[],
  labels: ReadonlyMap<string, TabLabel>,
  storage?: Pick<Storage, "setItem">,
): boolean {
  const allowed = new Set(decodeWorkspaceSessions(sessions));
  if (labels.size > 64) throw Error("workspace.labels_invalid");
  const entries = [];
  for (const [session, value] of labels) {
    decodeWorkspaceSessions([session]);
    if (!allowed.has(session)) throw Error("workspace.label_stale");
    entries.push({ session, ...tabLabel(value.title, value.color) });
  }
  try {
    storage?.setItem(KEY, JSON.stringify({ version: 1, entries }));
    return !!storage;
  } catch {
    return false;
  }
}
