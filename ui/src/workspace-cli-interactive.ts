import { uiText } from "./workspace-language";
import {
  answerInteractivePrompt,
  type InteractivePrompt,
} from "./workspace-ssh-interactive";

/** Render literal server questions inside the current original-window CLI request. */
export async function answerCliInteractive(
  doc: Document,
  prompt: InteractivePrompt,
  signal: AbortSignal,
): Promise<string[] | undefined> {
  if (signal.aborted) return undefined;
  const dialog = doc.createElement("dialog"),
    title = doc.createElement("h2");
  dialog.className = "terminal-paste-dialog workspace-ssh-interactive";
  title.id = "workspace-cli-interactive-title";
  uiText(title, "SSH 交互认证");
  dialog.setAttribute("aria-labelledby", title.id);
  dialog.append(title);
  doc.body.append(dialog);
  const controller = new AbortController();
  const abort = () => controller.abort();
  const cancel = (event: Event) => {
    event.preventDefault();
    abort();
  };
  signal.addEventListener("abort", abort, { once: true });
  dialog.addEventListener("cancel", cancel);
  dialog.addEventListener("close", abort);
  try {
    dialog.showModal();
    return await answerInteractivePrompt(dialog, prompt, controller.signal);
  } finally {
    signal.removeEventListener("abort", abort);
    dialog.removeEventListener("cancel", cancel);
    dialog.removeEventListener("close", abort);
    controller.abort();
    dialog.close();
    dialog.remove();
  }
}
