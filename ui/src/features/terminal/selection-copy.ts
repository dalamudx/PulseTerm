/** Only a trusted selection gesture can request the existing text/plain copy path. */
export function bindAutoCopy(
  root: HTMLElement | undefined,
  options: Readonly<{
    enabled: boolean;
    current(): boolean;
    selected(): boolean;
    copy(): void;
  }>,
): () => void {
  if (!options.enabled || !root) return () => {};
  const doc = root.ownerDocument;
  let pointer: number | undefined,
    disposed = false;
  const visible = () => !disposed && options.current() && !doc.hidden;
  const down = (event: Event) => {
    pointer = undefined;
    const e = event as PointerEvent;
    if (
      !e.isTrusted ||
      !visible() ||
      e.button !== 0 ||
      !(e.buttons & 1) ||
      (e.pointerType !== "mouse" && e.pointerType !== "pen") ||
      !Number.isSafeInteger(e.pointerId) ||
      e.pointerId < 0 ||
      (e.target !== root && !root.contains(e.target as Node | null))
    )
      return;
    pointer = e.pointerId;
  };
  const up = (event: Event) => {
    const original = pointer;
    pointer = undefined;
    const e = event as PointerEvent;
    if (
      original === undefined ||
      !e.isTrusted ||
      e.button !== 0 ||
      e.pointerId !== original ||
      !visible() ||
      !options.selected()
    )
      return;
    try {
      options.copy();
    } catch {
      /* A browser rejection cannot fall back to raw clipboard data. */
    }
  };
  const cancel = () => {
    pointer = undefined;
  };
  root.addEventListener("pointerdown", down, true);
  doc.addEventListener("pointerup", up, true);
  doc.addEventListener("pointercancel", cancel, true);
  doc.addEventListener("visibilitychange", cancel, true);
  doc.defaultView?.addEventListener("blur", cancel, true);
  root.addEventListener("focusout", cancel, true);
  return () => {
    disposed = true;
    pointer = undefined;
    root.removeEventListener("pointerdown", down, true);
    doc.removeEventListener("pointerup", up, true);
    doc.removeEventListener("pointercancel", cancel, true);
    doc.removeEventListener("visibilitychange", cancel, true);
    doc.defaultView?.removeEventListener("blur", cancel, true);
    root.removeEventListener("focusout", cancel, true);
  };
}
