// Retirement is local resource control, not a backend permission or session close.
export function retireBeforeReload(
  life: AbortController,
  dispose: () => void,
  navigate: () => void,
): void {
  if (life.signal.aborted) return;
  life.abort();
  dispose();
  navigate();
}
export function workspaceReloadKey(
  event: Pick<
    KeyboardEvent,
    "ctrlKey" | "shiftKey" | "altKey" | "metaKey" | "key"
  >,
): boolean {
  return (
    event.ctrlKey &&
    event.shiftKey &&
    !event.altKey &&
    !event.metaKey &&
    event.key.toLowerCase() === "r"
  );
}
