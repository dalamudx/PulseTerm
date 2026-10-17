import { isWorkspaceNonce } from "../../workspace-readiness";
export async function readSystemClipboard(
  doc: Document,
  signal: AbortSignal,
  current: () => boolean,
): Promise<string> {
  const clipboard = doc.defaultView?.navigator.clipboard;
  if (!clipboard?.readText) throw Error("paste.clipboard_api_missing");
  if (signal.aborted || !current()) throw Error("paste.cancelled");
  const text = await clipboard.readText();
  if (signal.aborted || !current()) throw Error("paste.cancelled");
  if (typeof text !== "string") throw Error("paste.clipboard_unavailable");
  return text;
}

export async function readPreparedClipboard(
  signal: AbortSignal,
  current: () => boolean,
  ports: {
    prepare(): Promise<unknown>;
    read(grant: string): Promise<unknown>;
    cancel(grant: string): Promise<unknown>;
  },
): Promise<string> {
  if (signal.aborted || !current()) throw Error("paste.cancelled");
  let grant: string | undefined;
  let released = false;
  const retire = () => {
    if (grant && !released) {
      released = true;
      void ports.cancel(grant).catch(() => undefined);
    }
  };
  signal.addEventListener("abort", retire, { once: true });
  try {
    const token = await ports.prepare();
    if (typeof token !== "string" || !isWorkspaceNonce(token))
      throw Error("paste.clipboard_unavailable");
    grant = token;
    if (signal.aborted || !current()) throw Error("paste.cancelled");
    const text = await ports.read(grant);
    if (signal.aborted || !current()) throw Error("paste.cancelled");
    if (typeof text !== "string") throw Error("paste.clipboard_unavailable");
    return text;
  } finally {
    signal.removeEventListener("abort", retire);
    retire();
  }
}
