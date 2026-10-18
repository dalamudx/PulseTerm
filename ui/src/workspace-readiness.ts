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
  const deadline = performance.now() + 5000;
  let timer: ReturnType<typeof setTimeout> | undefined;
  let stop!: () => void;
  const stopped = new Promise<"cancelled" | "denied">((resolve) => {
    stop = () => resolve("cancelled");
    signal.addEventListener("abort", stop, { once: true });
    timer = setTimeout(() => resolve("denied"), 5000);
  });
  try {
    // Ending the wait neither cancels native work nor accepts a late receipt.
    const result = await Promise.race([
      send(nonce).then(() => "ready" as const),
      stopped,
    ]);
    if (signal.aborted) return "cancelled";
    return performance.now() >= deadline ? "denied" : result;
  } catch {
    return signal.aborted ? "cancelled" : "denied";
  } finally {
    clearTimeout(timer);
    signal.removeEventListener("abort", stop);
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
