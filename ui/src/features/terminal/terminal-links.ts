import type { ILink, ILinkProvider, Terminal } from "@xterm/xterm";
import { reviewableFilePath } from "./terminal-file-links";
export const MAX_LINK_BYTES = 4096;
/** Conservative destination projection; never fetch or navigate. */
export function reviewableLink(raw: string): URL | undefined {
  if (
    !/^https?:\/\//i.test(raw) ||
    raw.length > MAX_LINK_BYTES ||
    new TextEncoder().encode(raw).length > MAX_LINK_BYTES ||
    /[\s\\]/u.test(raw) ||
    Array.from(raw).some((char) => {
      const cp = char.codePointAt(0) ?? 0;
      return (
        cp < 32 ||
        (cp >= 127 && cp <= 159) ||
        (cp >= 0x202a && cp <= 0x202e) ||
        (cp >= 0x2066 && cp <= 0x2069)
      );
    })
  )
    return;
  try {
    const url = new URL(raw);
    if (
      !url.hostname ||
      url.username ||
      url.password ||
      !["http:", "https:"].includes(url.protocol) ||
      new TextEncoder().encode(url.href).length > MAX_LINK_BYTES
    )
      return;
    return url;
  } catch {
    return;
  }
}
/** Explicit IP endpoint interpreted only as a candidate HTTP web address. */
export function endpointLink(raw: string): URL | undefined {
  if (raw.length > MAX_LINK_BYTES) return;
  const match =
    /^(\d{1,3}(?:\.\d{1,3}){3}|\[[\da-f:.]+\]):([1-9]\d{0,4})([/?#].*)?$/iu.exec(
      raw,
    );
  if (!match || Number(match[2]) > 65535) return;
  const host = match[1];
  if (!host) return;
  if (
    !host.startsWith("[") &&
    host
      .split(".")
      .some((part) => Number(part) > 255 || String(Number(part)) !== part)
  )
    return;
  return reviewableLink(`http://${raw}`);
}
function trimPunctuation(text: string): string {
  let result = text.replace(/[.,;!。，；！、]+$/u, "");
  for (const [open, close] of [
    ["(", ")"],
    ["[", "]"],
    ["{", "}"],
  ] as const) {
    const extra = result.split(close).length - result.split(open).length;
    let end = result.length;
    for (let i = 0; i < extra && result[end - 1] === close; i++) end--;
    result = result.slice(0, end);
  }
  return result;
}
/** Public buffer cells keep UTF-16 URL offsets aligned with wide/combined cells. */
export function textLinkProvider(
  terminal: Pick<Terminal, "buffer">,
  current: () => boolean,
  activate: (event: MouseEvent, uri: string, source?: "file") => void,
): ILinkProvider {
  return {
    provideLinks(row, callback) {
      if (!current() || !Number.isSafeInteger(row) || row < 1)
        return callback(undefined);
      const buffer = terminal.buffer.active;
      let first = row - 1;
      while (buffer.getLine(first)?.isWrapped) {
        if (row - first >= 8 || first === 0) return callback(undefined);
        first--;
      }
      const starts: { x: number; y: number }[] = [],
        ends: { x: number; y: number }[] = [];
      let text = "";
      for (let y = first; ; y++) {
        const line = buffer.getLine(y),
          next = buffer.getLine(y + 1);
        if (!line || line.length > 4096 || y - first >= 8)
          return callback(undefined);
        for (let x = 0; x < line.length; x++) {
          const cell = line.getCell(x);
          if (!cell || cell.getWidth() === 0) continue;
          if (
            x === line.length - 1 &&
            !cell.getCode() &&
            next?.isWrapped &&
            next.getCell(0)?.getWidth() === 2
          )
            continue;
          const chars = cell.getChars() || " ";
          if (text.length + chars.length > 16384) return callback(undefined);
          for (let j = 0; j < chars.length; j++) {
            starts.push({ x: x + 1, y: y + 1 });
            ends.push({ x: x + cell.getWidth(), y: y + 1 });
          }
          text += chars;
        }
        if (!next?.isWrapped) break;
      }
      const links: ILink[] = [];
      let candidates = 0;
      let quote = "",
        scanned = 0,
        escaped = false;
      for (const match of text.matchAll(
        /"(?:\/|[a-z]:[\\/])[^"\r\n]*"(?::[0-9]+(?::[0-9]+)?)?|'(?:\/|[a-z]:[\\/])[^'\r\n]*'(?::[0-9]+(?::[0-9]+)?)?|https?:\/\/[^\s<>"'`]+|(?:\d{1,3}(?:\.\d{1,3}){3}|\[[\da-f:.]+\]):[^\s<>"'`]+|(?:\/|[a-z]:[\\/])[^\s<>"'`]+/giu,
      )) {
        if (++candidates > 32) break;
        // Scan each prefix once so nested or incomplete quotes cannot yield partial paths.
        for (; scanned < match.index; scanned++) {
          const char = text.charAt(scanned);
          if (escaped) {
            escaped = false;
            continue;
          }
          if (quote && quote !== "'" && char === "\\") {
            escaped = true;
            continue;
          }
          if (quote) {
            if (char === quote) quote = "";
          } else if (
            (char === "'" || char === '"' || char === "`") &&
            (scanned === 0 || /[\s=([{,;]/u.test(text.charAt(scanned - 1)))
          )
            quote = char;
        }
        const quotedPath = match[0].startsWith('"') || match[0].startsWith("'");
        if (quotedPath) {
          if (quote) continue;
          // This whole literal token has already been consumed, including backslashes.
          scanned = match.index + match[0].length;
          escaped = false;
          const after = text.charAt(scanned);
          if (after && !/[\s)\]},;，；。.!?]/u.test(after)) continue;
        }
        const pathCandidate =
          quotedPath || /^(?:\/|[a-z]:[\\/])/iu.test(match[0]);
        let raw = pathCandidate ? match[0] : trimPunctuation(match[0]);
        if (pathCandidate) {
          const before = text.charAt(match.index - 1);
          const close =
            before === "("
              ? ")"
              : before === "["
                ? "]"
                : before === "{"
                  ? "}"
                  : undefined;
          const enclosed = raw.replace(/[.,;!。，；！、]+$/u, "");
          if (
            close &&
            enclosed.endsWith(close) &&
            enclosed.split(close).length > enclosed.split(before).length
          )
            raw = enclosed.slice(0, -1);
        }
        const explicit = /^https?:\/\//i.test(raw);
        if (!explicit && quote) continue;
        if (
          !explicit &&
          match.index > 0 &&
          !/[\s([{=,;，；：]/u.test(text.charAt(match.index - 1))
        )
          continue;
        const target = explicit ? reviewableLink(raw) : endpointLink(raw);
        const file = target ? undefined : reviewableFilePath(raw);
        if (!target && !file) continue;
        const start = starts[match.index],
          end = ends[match.index + raw.length - 1];
        if (!start || !end || row < start.y || row > end.y) continue;
        links.push({
          text: raw,
          range: { start, end },
          activate: (event) => {
            event.preventDefault();
            if (!current()) return;
            if (file) activate(event, raw, "file");
            else if (target) activate(event, explicit ? raw : target.href);
          },
        });
      }
      callback(current() ? links : undefined);
    },
  };
}
