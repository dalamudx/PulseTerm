const UUID_V4 =
  /^[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}$/i;

export function isWorkspaceNonce(value: string): boolean {
  return UUID_V4.test(value);
}

/** Local display receipt only. Backend validates the actual window, origin, and page nonce. */
export async function confirmWorkspaceDocument(
  nonce: string,
  send: (nonce: string) => Promise<unknown>,
  signal: AbortSignal,
): Promise<"ready" | "cancelled" | "denied"> {
  if (signal.aborted) return "cancelled";
  if (!isWorkspaceNonce(nonce)) return "denied";
  try {
    await send(nonce);
    return signal.aborted ? "cancelled" : "ready";
  } catch {
    return signal.aborted ? "cancelled" : "denied";
  }
}

/** Reports original-page initialization only; never confirms a page or admits an action. */
export async function reportWorkspacePresentation(
  nonce: string,
  send: (nonce: string) => Promise<unknown>,
  signal: AbortSignal,
  current: () => string | undefined,
): Promise<"ready" | "cancelled" | "denied"> {
  if (signal.aborted || current() !== nonce) return "cancelled";
  const result = await confirmWorkspaceDocument(nonce, send, signal);
  return signal.aborted || current() !== nonce ? "cancelled" : result;
}
