export interface TerminalFileTarget {
  readonly path: string;
  readonly style: "unix" | "windows";
  readonly line?: number;
  readonly column?: number;
}
/** A location in terminal text, never a local filesystem capability. */
export function reviewableFilePath(
  raw: string,
): TerminalFileTarget | undefined {
  if (
    raw.length > 4096 ||
    new TextEncoder().encode(raw).length > 4096 ||
    /[\p{Cc}\p{Cf}\p{Cs}]/u.test(raw)
  )
    return;
  const quoted = raw.startsWith('"') || raw.startsWith("'");
  const token = quoted
    ? /^(["'])(.*?)\1(?::([0-9]+)(?::([0-9]+))?)?$/.exec(raw)
    : undefined;
  if (quoted && !token) return;
  const literal = token?.[2] ?? raw;
  // Quotes delimit one literal token; no shell escapes or variable expansion.
  if (/[<>"'`]/u.test(literal) || (quoted ? /[^\S ]/u : /\s/u).test(literal))
    return;
  const externalPosition = token?.[3] !== undefined;
  const position = externalPosition
    ? undefined
    : /^(.*?):([0-9]+)(?::([0-9]+))?$/.exec(literal);
  const path = position?.[1] ?? literal;
  const windows = /^[a-z]:[\\/]/i.test(path);
  if (
    windows
      ? /[:|?*]/u.test(path.slice(2))
      : !path.startsWith("/") || path.startsWith("//") || path.includes("\\")
  )
    return;
  // Do not turn endpoint-shaped fragments into paths or source positions.
  if (/^\/\d{1,3}(?:\.\d{1,3}){3}(?::\d+){1,2}$/u.test(literal)) return;
  const parts = (
    windows ? path.slice(3).replaceAll("\\", "/") : path.slice(1)
  ).split("/");
  if (parts.some((part) => !part || part === "." || part === "..")) return;
  const lineText = externalPosition ? token?.[3] : position?.[2];
  const columnText = externalPosition ? token?.[4] : position?.[3];
  const line = lineText === undefined ? undefined : Number(lineText);
  const column = columnText === undefined ? undefined : Number(columnText);
  if (
    [line, column].some(
      (n) =>
        n !== undefined &&
        (!Number.isSafeInteger(n) || n < 1 || n > 2147483647),
    )
  )
    return;
  return {
    path,
    style: windows ? "windows" : "unix",
    ...(line === undefined ? {} : { line }),
    ...(column === undefined ? {} : { column }),
  };
}
