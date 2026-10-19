import { isCompositionKey } from "./keybindings/composition";

export function focusedWorkspaceModal(
  document: Pick<Document, "activeElement" | "querySelector">,
): HTMLDialogElement | null {
  return (
    document.activeElement?.closest<HTMLDialogElement>("dialog[open]") ??
    document.querySelector<HTMLDialogElement>("dialog[open]")
  );
}
/** Modal dismissal consumes the whole Escape gesture before focus can return to xterm. */
export class WorkspaceModalEscape {
  private suppressing = false;
  constructor(
    private readonly signal: AbortSignal,
    private readonly modal: () => HTMLDialogElement | null,
  ) {}
  keydown(event: KeyboardEvent): boolean {
    if (
      this.signal.aborted ||
      event.key !== "Escape" ||
      isCompositionKey(event) ||
      event.ctrlKey ||
      event.metaKey ||
      event.altKey ||
      event.shiftKey ||
      event.defaultPrevented
    )
      return false;
    const dialog = this.modal();
    if (!dialog && !this.suppressing) return false;
    event.preventDefault();
    event.stopImmediatePropagation();
    const first = !this.suppressing;
    this.suppressing = true;
    if (dialog && first && !event.repeat) {
      const cancel = new Event("cancel", { cancelable: true });
      // Existing controllers own cancellation/receipts. Native default closing is
      // only used for a modal with no controller-provided cancel handler.
      if (dialog.dispatchEvent(cancel) && dialog.open) dialog.close();
    }
    return true;
  }
  keyup(event: KeyboardEvent): boolean {
    if (this.signal.aborted || event.key !== "Escape" || !this.suppressing)
      return false;
    this.suppressing = false;
    event.preventDefault();
    event.stopImmediatePropagation();
    return true;
  }
}
