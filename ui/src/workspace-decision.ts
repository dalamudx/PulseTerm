import { type UiMessage, uiAttribute, uiText } from "./workspace-language";
export interface DecisionCopy {
  title: string | UiMessage;
  description: string | UiMessage;
  accept: string | UiMessage;
  cancel: string | UiMessage;
  preview?: string | UiMessage;
  previewLabel?: string | UiMessage;
}
const pending = new WeakSet<Document>();
/** Native modal owns focus; Cancel is always the default. One decision per document. */
export function workspaceDecision(
  doc: Document,
  copy: DecisionCopy,
  signal: AbortSignal,
): Promise<boolean> {
  if (signal.aborted || pending.has(doc)) return Promise.resolve(false);
  pending.add(doc);
  return new Promise<boolean>((resolve) => {
    const dialog = doc.createElement("dialog"),
      title = doc.createElement("h2"),
      description = doc.createElement("p"),
      actions = doc.createElement("div"),
      cancel = doc.createElement("button"),
      accept = doc.createElement("button");
    dialog.className = "terminal-paste-dialog";
    dialog.setAttribute("aria-labelledby", "workspace-decision-title");
    dialog.setAttribute("aria-describedby", "workspace-decision-description");
    title.id = "workspace-decision-title";
    uiText(title, copy.title);
    description.id = "workspace-decision-description";
    uiText(description, copy.description);
    dialog.append(title, description);
    if (copy.preview !== undefined) {
      const preview = doc.createElement("textarea");
      preview.readOnly = true;
      preview.rows = 6;
      preview.wrap = "off";
      if (typeof copy.preview === "string") preview.value = copy.preview;
      else uiAttribute(preview, "value", copy.preview);
      uiAttribute(preview, "aria-label", copy.previewLabel ?? copy.title);
      dialog.append(preview);
    }
    actions.className = "terminal-paste-actions";
    cancel.type = "button";
    cancel.autofocus = true;
    uiText(cancel, copy.cancel);
    accept.type = "button";
    uiText(accept, copy.accept);
    accept.className = "terminal-paste-accept";
    actions.append(cancel, accept);
    dialog.append(actions);
    let finished = false;
    const finish = (allowed: boolean) => {
      if (finished) return;
      finished = true;
      signal.removeEventListener("abort", abort);
      dialog.removeEventListener("cancel", onCancel);
      dialog.removeEventListener("close", closed);
      cancel.removeEventListener("click", reject);
      accept.removeEventListener("click", approve);
      if (dialog.open) dialog.close();
      dialog.remove();
      resolve(allowed && !signal.aborted);
    };
    const abort = () => finish(false),
      reject = () => finish(false),
      approve = () => finish(true),
      closed = () => finish(false),
      onCancel = (event: Event) => {
        event.preventDefault();
        finish(false);
      };
    cancel.addEventListener("click", reject);
    accept.addEventListener("click", approve);
    dialog.addEventListener("cancel", onCancel);
    dialog.addEventListener("close", closed);
    signal.addEventListener("abort", abort, { once: true });
    doc.body.append(dialog);
    try {
      dialog.showModal();
      if (signal.aborted) finish(false);
      else cancel.focus();
    } catch {
      finish(false);
    }
  }).finally(() => pending.delete(doc));
}
